#![allow(clippy::cast_sign_loss)]

use hexo_engine::board::Board;
use hexo_engine::config::{MAX_PIECE_DISTANCE, MOVE_GEN_INNER_RADIUS};
use hexo_engine::coords::{Coord, ORIGIN, for_each_in_range, hex_distance};
use hexo_engine::generate;
use std::collections::HashSet;

fn place_ok(b: &mut Board, c: Coord) {
    b.place(c)
        .unwrap_or_else(|e| panic!("place({c:?}) failed: {e:?}"));
}

/// Set of empty cells within `radius` of any placed piece (excluding pieces).
fn expected_neighbourhood(b: &Board, radius: i16) -> HashSet<Coord> {
    let pieces: HashSet<Coord> = b.pieces().map(|(c, _)| c).collect();
    let mut out = HashSet::new();
    for &p in &pieces {
        for_each_in_range(p, radius, |d| {
            if d != p && !pieces.contains(&d) {
                out.insert(d);
            }
        });
    }
    out
}

#[test]
fn empty_board_returns_origin() {
    let b = Board::new();
    let moves: Vec<Coord> = generate(&b, MOVE_GEN_INNER_RADIUS).into_iter().collect();
    assert_eq!(moves, vec![ORIGIN]);
}

#[test]
fn empty_board_origin_independent_of_radius() {
    let b = Board::new();
    for r in [1i16, 2, 4, 8, 100] {
        let moves: Vec<Coord> = generate(&b, r).into_iter().collect();
        assert_eq!(moves, vec![ORIGIN], "radius {r}");
    }
}

#[test]
fn single_piece_inner_radius() {
    let mut b = Board::new();
    place_ok(&mut b, ORIGIN);
    let moves: HashSet<Coord> = generate(&b, MOVE_GEN_INNER_RADIUS).into_iter().collect();
    let r = MOVE_GEN_INNER_RADIUS;
    let expected_count = 3 * r as usize * (r as usize + 1);
    assert_eq!(moves.len(), expected_count);
    assert!(!moves.contains(&ORIGIN));
    for m in &moves {
        let d = hex_distance(*m, ORIGIN);
        assert!(d >= 1 && d <= r, "{m:?} at dist {d} outside [1,{r}]");
    }
}

#[test]
fn single_piece_outer_radius() {
    let mut b = Board::new();
    place_ok(&mut b, ORIGIN);
    // Pick a radius strictly larger than INNER to exercise the outer path.
    let r = MOVE_GEN_INNER_RADIUS + 2;
    let moves: HashSet<Coord> = generate(&b, r).into_iter().collect();
    let expected_count = 3 * r as usize * (r as usize + 1);
    assert_eq!(moves.len(), expected_count);
    assert!(!moves.contains(&ORIGIN));
    for m in &moves {
        let d = hex_distance(*m, ORIGIN);
        assert!(d >= 1 && d <= r, "{m:?} at dist {d} outside [1,{r}]");
    }
}

#[test]
fn outer_excludes_occupied() {
    let mut b = Board::new();
    place_ok(&mut b, ORIGIN);
    let extra = Coord::new(3, 0);
    place_ok(&mut b, extra);
    let moves: HashSet<Coord> = generate(&b, 4).into_iter().collect();
    assert!(!moves.contains(&ORIGIN));
    assert!(!moves.contains(&extra));
}

#[test]
fn outer_dedupes() {
    let mut b = Board::new();
    place_ok(&mut b, ORIGIN);
    place_ok(&mut b, Coord::new(1, 0));
    let raw = generate(&b, MOVE_GEN_INNER_RADIUS + 1);
    let dedup: HashSet<Coord> = raw.iter().copied().collect();
    assert_eq!(raw.len(), dedup.len(), "duplicate move emitted");
}

#[test]
fn outer_matches_independent_neighbourhood() {
    // Forward-sweep result matches a freshly computed reference set.
    let mut b = Board::new();
    place_ok(&mut b, ORIGIN);
    place_ok(&mut b, Coord::new(2, 0));
    place_ok(&mut b, Coord::new(-1, 1));
    let r = MOVE_GEN_INNER_RADIUS + 1;
    let moves: HashSet<Coord> = generate(&b, r).into_iter().collect();
    let expected = expected_neighbourhood(&b, r);
    assert_eq!(moves, expected);
}

#[test]
fn inner_path_matches_independent_neighbourhood() {
    let mut b = Board::new();
    place_ok(&mut b, ORIGIN);
    place_ok(&mut b, Coord::new(2, 0));
    place_ok(&mut b, Coord::new(-1, 1));
    let moves: HashSet<Coord> = generate(&b, MOVE_GEN_INNER_RADIUS).into_iter().collect();
    let expected = expected_neighbourhood(&b, MOVE_GEN_INNER_RADIUS);
    assert_eq!(moves, expected);
}

#[test]
fn outer_clamps_to_max() {
    let mut b = Board::new();
    place_ok(&mut b, ORIGIN);
    let at_max: HashSet<Coord> = generate(&b, MAX_PIECE_DISTANCE).into_iter().collect();
    let above_max: HashSet<Coord> = generate(&b, MAX_PIECE_DISTANCE + 100).into_iter().collect();
    let way_above: HashSet<Coord> = generate(&b, 1000).into_iter().collect();
    assert_eq!(at_max, above_max);
    assert_eq!(at_max, way_above);
}

#[test]
fn outer_path_many_pieces_dedupes_and_excludes_occupied() {
    // Pack ~12 stones along the q axis and verify generate(r=4) produces a
    // dedup'd set of empty cells. Exercises heavy overlap and the
    // saturating reserve heuristic.
    let mut b = Board::new();
    place_ok(&mut b, ORIGIN);
    let mut placed: HashSet<Coord> = HashSet::from([ORIGIN]);
    // Place on alternating sides of the origin to spread the cluster.
    for k in 1..=6 {
        let pos = Coord::new(k, 0);
        place_ok(&mut b, pos);
        placed.insert(pos);
        let neg = Coord::new(-k, 0);
        place_ok(&mut b, neg);
        placed.insert(neg);
    }
    let r = 4;
    let raw = generate(&b, r);
    let unique: HashSet<Coord> = raw.iter().copied().collect();
    assert_eq!(raw.len(), unique.len(), "duplicates returned");
    for m in &unique {
        assert!(!placed.contains(m), "{m:?} is occupied");
    }
    // Sanity: union must equal the reference forward-sweep set.
    let expected = expected_neighbourhood(&b, r);
    assert_eq!(unique, expected);
}

#[test]
fn radius_below_inner_uses_inner() {
    // Spec: `radius <= MOVE_GEN_INNER_RADIUS` returns the inner candidate set.
    // For a one-piece board, that set is the full inner neighbourhood,
    // regardless of whether the caller passes a smaller radius.
    let mut b = Board::new();
    place_ok(&mut b, ORIGIN);
    let moves: HashSet<Coord> = generate(&b, 1).into_iter().collect();
    let r = MOVE_GEN_INNER_RADIUS;
    let expected_count = 3 * r as usize * (r as usize + 1);
    assert_eq!(moves.len(), expected_count);
}
