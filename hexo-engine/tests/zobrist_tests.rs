//! Phase 6 zobrist tests: per-cell key determinism + halfmove parity.

#![allow(clippy::cast_possible_truncation)]

use fxhash::FxHashSet;
use hexo_engine_core::coords::ORIGIN;
use hexo_engine_core::zobrist::{Z_HALFMOVE, Z_TURN_X, ZobristTable};
use hexo_engine_core::{Board, Coord, Player};
use rand_xoshiro::Xoshiro256PlusPlus;
use rand_xoshiro::rand_core::{Rng, SeedableRng};

/// 1) Per-cell key generation must stay deterministic across `ZobristTable`
///    instances. Two tables built back-to-back must produce identical keys
///    for any `(c, p)` — guards the byte-identical-keys invariant against
///    accidental seed-stream changes.
#[test]
fn zobrist_keys_deterministic_across_instances() {
    let mut a = ZobristTable::new();
    let mut b = ZobristTable::new();

    for &(q, r) in &[(0, 0), (3, -1), (-7, 2), (127, 127), (-127, -127)] {
        let c = Coord::new(q, r);
        for &p in &[Player::X, Player::O] {
            let ka = a.key(c, p);
            let kb = b.key(c, p);
            assert_eq!(ka, kb, "key({c:?}, {p:?}) differs across instances");
            assert_ne!(ka, 0, "key({c:?}, {p:?}) is zero");
        }
    }
}

/// 2) `Z_TURN_X` and `Z_HALFMOVE` are nonzero, distinct from each other,
///    and distinct from every per-cell key in the first ~2 000 sampled
///    coords.
#[test]
fn parity_constants_unique() {
    assert_ne!(Z_TURN_X, 0);
    assert_ne!(Z_HALFMOVE, 0);
    assert_ne!(Z_TURN_X, Z_HALFMOVE);

    let mut tbl = ZobristTable::new();
    for q in -16..=16i16 {
        for r in -16..=16i16 {
            for &p in &[Player::X, Player::O] {
                let k = tbl.key(Coord::new(q, r), p);
                assert_ne!(k, Z_TURN_X, "({q},{r},{p:?}) collides with Z_TURN_X");
                assert_ne!(k, Z_HALFMOVE, "({q},{r},{p:?}) collides with Z_HALFMOVE");
            }
        }
    }
}

/// 3) The empty-board hash is fixed across runs. We pin it to the exact
///    parity overlay (`Z_TURN_X`) rather than a bare integer so future
///    parity-constant tweaks fail loudly in a single, obvious spot.
#[test]
fn empty_board_hash_is_z_turn_x() {
    let b = Board::new();
    assert_eq!(b.hash(), Z_TURN_X);
    assert_eq!(b.halfmove(), 0);
    assert_eq!(b.to_move(), Player::X);
}

/// 4) Place/undo round trip restores the prior hash exactly. Tests over
///    a 10-stone deterministic walk drawn from a fixed PRNG sequence.
#[test]
fn place_undo_round_trip_preserves_hash() {
    let mut b = Board::new();
    let mut hashes: Vec<u128> = vec![b.hash()];
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(0xABCD_1234_5678_FFFF);

    let mut stones: Vec<Coord> = Vec::new();
    // Walk: first move at origin, then 9 random-but-legal moves.
    b.place(ORIGIN).unwrap();
    stones.push(ORIGIN);
    hashes.push(b.hash());

    for _ in 0..9 {
        let cands: Vec<Coord> = b.candidates().collect();
        let pick = cands[(rng.next_u64() as usize) % cands.len()];
        b.place(pick).unwrap();
        stones.push(pick);
        hashes.push(b.hash());
    }

    for _ in 0..stones.len() {
        b.undo().unwrap();
    }
    assert_eq!(b.hash(), hashes[0], "hash drift after full undo");
    assert_eq!(b.ply(), 0);
}

/// 5) Halfmove transitions across the first six plies follow the
///    `HeXO` opening rule: X plays a singleton, then full O/X/O/X turn
///    pairs.
#[test]
fn halfmove_transitions_opening() {
    let mut b = Board::new();
    assert_eq!((b.to_move(), b.halfmove()), (Player::X, 0), "ply 0");

    b.place(ORIGIN).unwrap();
    assert_eq!(
        (b.to_move(), b.halfmove()),
        (Player::O, 0),
        "after X singleton"
    );

    b.place(Coord::new(1, 0)).unwrap();
    assert_eq!((b.to_move(), b.halfmove()), (Player::O, 1), "O stone 1");

    b.place(Coord::new(2, 0)).unwrap();
    assert_eq!((b.to_move(), b.halfmove()), (Player::X, 0), "after O turn");

    b.place(Coord::new(3, 0)).unwrap();
    assert_eq!(
        (b.to_move(), b.halfmove()),
        (Player::X, 1),
        "X stone 1 of turn"
    );

    b.place(Coord::new(4, 0)).unwrap();
    assert_eq!((b.to_move(), b.halfmove()), (Player::O, 0), "after X turn");
}

/// 6) Structural parity bit: two positions with the same occupancy but
///    differing `(side, halfmove)` must hash differently. Uses the
///    test-only `force_parity_for_test` setter to construct the
///    hypothetical mid-turn state of X.
#[test]
fn parity_distinguishes_otherwise_identical_states() {
    // Real game-state after X's opening singleton: (O, 0).
    let mut real = Board::new();
    real.place(ORIGIN).unwrap();
    let real_hash = real.hash();
    assert_eq!(real.to_move(), Player::O);
    assert_eq!(real.halfmove(), 0);

    // Hypothetical state: same occupancy, but (X, 1) — "X about to play
    // stone 2" — unreachable from a real game but structurally distinct.
    let mut hypo = Board::new();
    hypo.place(ORIGIN).unwrap();
    hypo.force_parity_for_test(Player::X, 1);

    assert_ne!(real_hash, hypo.hash(), "parity overlay collapsed");

    // Cross-check the four parity combinations are pairwise distinct.
    let mut overlays: Vec<u128> = Vec::new();
    for &(side, half) in &[
        (Player::X, 0),
        (Player::X, 1),
        (Player::O, 0),
        (Player::O, 1),
    ] {
        let mut h = Board::new();
        h.place(ORIGIN).unwrap();
        h.force_parity_for_test(side, half);
        overlays.push(h.hash());
    }
    let set: FxHashSet<u128> = overlays.iter().copied().collect();
    assert_eq!(set.len(), 4, "four parity states must hash distinctly");
}

/// 7) Hash uniqueness over a 50-stone deterministic game. With a
///    128-bit key, a legitimate collision is ~10⁻³⁵; any duplicate
///    here means a parity or per-cell bug.
#[test]
fn hash_unique_across_50_stone_game() {
    let mut b = Board::new();
    let mut seen: FxHashSet<u128> = FxHashSet::default();
    seen.insert(b.hash());
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(0x1357_2468_ACE0_BDF1);

    b.place(ORIGIN).unwrap();
    assert!(seen.insert(b.hash()), "duplicate hash at ply 1");

    for ply in 2..=50 {
        let cands: Vec<Coord> = b.candidates().collect();
        assert!(!cands.is_empty(), "no candidates at ply {ply}");
        let pick = cands[(rng.next_u64() as usize) % cands.len()];
        b.place(pick).unwrap();
        assert!(seen.insert(b.hash()), "duplicate hash at ply {ply}");
    }
    assert_eq!(
        seen.len(),
        51,
        "50 stones + empty board = 51 distinct hashes"
    );
}
