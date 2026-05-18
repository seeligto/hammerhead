use hexo_engine::board::{Board, Player};
use hexo_engine::coords::{Coord, ORIGIN};
use hexo_engine::win::is_winning_move;

fn place_ok(b: &mut Board, c: Coord) {
    b.place(c)
        .unwrap_or_else(|e| panic!("place({c:?}) failed: {e:?}"));
}

/// Plays 11 stones (6 X, 5 O) such that 5 X stones sit at positions
/// `1..=5` along `step` from origin. The caller plays the 6th X (and any
/// other final move) themselves to control what `is_winning_move` sees.
///
/// Parity: X plies are 0, 3, 4, 7, 8 (5 plies, 5 X stones). The 6 O plies
/// are 1, 2, 5, 6, 9, 10. We place 11 stones; the next ply (11) is X.
fn play_five_x_along(b: &mut Board, step: Coord, pad: (Coord, Coord)) {
    let line = |k: i16| Coord::new(step.q * k, step.r * k);
    let (p1, p2) = pad;
    let p_at = |k: i16, base: Coord| Coord::new(base.q + step.q * k, base.r + step.r * k);

    place_ok(b, line(0));
    place_ok(b, p_at(0, p1));
    place_ok(b, p_at(0, p2));
    place_ok(b, line(1));
    place_ok(b, line(2));
    place_ok(b, p_at(1, p1));
    place_ok(b, p_at(1, p2));
    place_ok(b, line(3));
    place_ok(b, line(4));
    place_ok(b, p_at(2, p1));
    place_ok(b, p_at(2, p2));
}

#[test]
fn winning_move_axis_q() {
    let mut b = Board::new();
    play_five_x_along(
        &mut b,
        Coord::new(1, 0),
        (Coord::new(0, 4), Coord::new(0, -4)),
    );
    // X stones currently at (0..=4, 0). Next X play extends to (5, 0).
    let c = Coord::new(5, 0);
    place_ok(&mut b, c);
    assert!(is_winning_move(&b, c, Player::X));
}

#[test]
fn winning_move_axis_r() {
    let mut b = Board::new();
    play_five_x_along(
        &mut b,
        Coord::new(0, 1),
        (Coord::new(4, 0), Coord::new(-4, 0)),
    );
    let c = Coord::new(0, 5);
    place_ok(&mut b, c);
    assert!(is_winning_move(&b, c, Player::X));
}

#[test]
fn winning_move_axis_s() {
    let mut b = Board::new();
    play_five_x_along(
        &mut b,
        Coord::new(1, -1),
        (Coord::new(0, 4), Coord::new(0, -4)),
    );
    let c = Coord::new(5, -5);
    place_ok(&mut b, c);
    assert!(is_winning_move(&b, c, Player::X));
}

#[test]
fn non_winning_move() {
    // Only 5 X stones in a row after the placement — not a win.
    let mut b = Board::new();
    let step = Coord::new(1, 0);
    let p1 = Coord::new(0, 4);
    let p2 = Coord::new(0, -4);

    place_ok(&mut b, ORIGIN); // X at (0,0)
    place_ok(&mut b, p1); // O
    place_ok(&mut b, p2); // O
    place_ok(&mut b, Coord::new(step.q, step.r)); // X at (1,0)
    place_ok(&mut b, Coord::new(2 * step.q, 2 * step.r)); // X at (2,0)
    place_ok(&mut b, Coord::new(1, 4)); // O
    place_ok(&mut b, Coord::new(1, -4)); // O
    place_ok(&mut b, Coord::new(3, 0)); // X
    // Last X is the 5th stone in axis Q; only 5 in a row.
    let c = Coord::new(4, 0);
    place_ok(&mut b, c);
    assert!(!is_winning_move(&b, c, Player::X));
}

#[test]
fn winning_move_blocked_by_opponent() {
    // X at 0,1,2,4,5; O at 3 splits the line. Placing X at 6 keeps the max
    // run at 3 — not a win.
    let mut b = Board::new();
    place_ok(&mut b, Coord::new(0, 0)); // X
    place_ok(&mut b, Coord::new(0, 4)); // O
    place_ok(&mut b, Coord::new(0, -4)); // O
    place_ok(&mut b, Coord::new(1, 0)); // X
    place_ok(&mut b, Coord::new(2, 0)); // X
    place_ok(&mut b, Coord::new(1, 4)); // O
    place_ok(&mut b, Coord::new(3, 0)); // O (block)
    place_ok(&mut b, Coord::new(4, 0)); // X
    place_ok(&mut b, Coord::new(5, 0)); // X
    place_ok(&mut b, Coord::new(2, 4)); // O
    place_ok(&mut b, Coord::new(2, -4)); // O
    let c = Coord::new(6, 0);
    place_ok(&mut b, c);
    assert!(!is_winning_move(&b, c, Player::X));
}

#[test]
fn winning_move_through_middle() {
    // Build X at 0,1,2,4,5,6 then bridge at 3 → 7-in-row.
    let mut b = Board::new();
    place_ok(&mut b, Coord::new(0, 0)); // X
    place_ok(&mut b, Coord::new(0, 4)); // O
    place_ok(&mut b, Coord::new(0, -4)); // O
    place_ok(&mut b, Coord::new(1, 0)); // X
    place_ok(&mut b, Coord::new(2, 0)); // X
    place_ok(&mut b, Coord::new(1, 4)); // O
    place_ok(&mut b, Coord::new(1, -4)); // O
    place_ok(&mut b, Coord::new(4, 0)); // X
    place_ok(&mut b, Coord::new(5, 0)); // X
    place_ok(&mut b, Coord::new(2, 4)); // O
    place_ok(&mut b, Coord::new(2, -4)); // O
    place_ok(&mut b, Coord::new(6, 0)); // X (ply 11)
    // ply 12 is also X. Bridge fills the gap at (3, 0).
    let c = Coord::new(3, 0);
    place_ok(&mut b, c);
    assert!(is_winning_move(&b, c, Player::X));
}
