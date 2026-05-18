//! Phase 7 ordering tests. Covers bucket priority, killer/history
//! feed-in, `MOVE_GEN_CAP` truncation, history decay, killer dedup.

use hexo_engine::board::{Board, Player};
use hexo_engine::config::{HISTORY_CUTOFF_MAX, MOVE_GEN_CAP};
use hexo_engine::coords::Coord;
use hexo_engine::moves::MoveList;
use hexo_engine::ordering::{KillerSlot, OrderingContext, OrderingState, order_moves};
use smallvec::SmallVec;

// ────────────────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────────────────

fn x_at(b: &mut Board, cells: &[(i16, i16)]) {
    for &(q, r) in cells {
        b.place_for_test(Coord::new(q, r), Player::X);
    }
}

fn o_at(b: &mut Board, cells: &[(i16, i16)]) {
    for &(q, r) in cells {
        b.place_for_test(Coord::new(q, r), Player::O);
    }
}

fn mv(q: i16, r: i16) -> Coord {
    Coord::new(q, r)
}

fn list(cells: &[Coord]) -> MoveList {
    let mut v: MoveList = SmallVec::new();
    v.extend(cells.iter().copied());
    v
}

fn ctx<'a>(
    board: &'a Board,
    side: Player,
    tt_move: Option<Coord>,
    killers: &'a KillerSlot,
    history: &'a fxhash::FxHashMap<(Coord, Player), u32>,
    stone1_s0_defense: &'a [Coord],
) -> OrderingContext<'a> {
    OrderingContext {
        board,
        side,
        tt_move,
        killers,
        history,
        stone1_s0_defense,
    }
}

// ────────────────────────────────────────────────────────────────────────
// 1. TT move wins
// ────────────────────────────────────────────────────────────────────────

#[test]
fn tt_move_ranks_first() {
    let mut b = Board::new();
    x_at(&mut b, &[(0, 0), (1, 0)]);
    let state = OrderingState::new();
    let tt = mv(20, 20);
    let killer = KillerSlot::default();
    let c = ctx(&b, Player::X, Some(tt), &killer, &state.history, &[]);
    let mut moves = list(&[mv(2, 0), tt, mv(3, 0), mv(-1, 0)]);
    order_moves(&mut moves, &c);
    assert_eq!(moves[0], tt, "TT move must rank first");
}

// ────────────────────────────────────────────────────────────────────────
// 2. Win-move beats S0
// ────────────────────────────────────────────────────────────────────────

#[test]
fn win_move_beats_s0_creator() {
    let mut b = Board::new();
    // X has 5 stones along q-axis at r=0; playing (5,0) wins.
    x_at(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)]);
    // Separate X cluster on r=5 line for S0 creator at (3,5).
    x_at(&mut b, &[(0, 5), (1, 5), (2, 5)]);
    let state = OrderingState::new();
    let killer = KillerSlot::default();
    let c = ctx(&b, Player::X, None, &killer, &state.history, &[]);
    let win = mv(5, 0);
    let s0_creator = mv(3, 5);
    let mut moves = list(&[s0_creator, win]);
    order_moves(&mut moves, &c);
    assert_eq!(moves[0], win, "winning move must beat S0 creator");
}

// ────────────────────────────────────────────────────────────────────────
// 3. Defensive win beats own S0 creation
// ────────────────────────────────────────────────────────────────────────

#[test]
fn defensive_win_beats_own_s0_creation() {
    let mut b = Board::new();
    // O has 5 stones along q-axis; (5,0) is the unique blocker.
    o_at(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)]);
    // X has 3 stones on r=5; playing (3,5) creates own open-4 (S0).
    x_at(&mut b, &[(0, 5), (1, 5), (2, 5)]);
    let state = OrderingState::new();
    let killer = KillerSlot::default();
    let c = ctx(&b, Player::X, None, &killer, &state.history, &[]);
    let block = mv(5, 0);
    let own_s0 = mv(3, 5);
    let mut moves = list(&[own_s0, block]);
    order_moves(&mut moves, &c);
    assert_eq!(moves[0], block, "defensive win must outrank own S0");
}

// ────────────────────────────────────────────────────────────────────────
// 4. Stone-1 S0 completion outranks creates-S0
// ────────────────────────────────────────────────────────────────────────

#[test]
fn stone1_s0_completion_outranks_creates_s0() {
    let mut b = Board::new();
    // Cluster on r=5 so (3,5) creates open-4 (creates_s0, bucket 6).
    x_at(&mut b, &[(0, 5), (1, 5), (2, 5)]);
    // Anchor stone to keep board non-empty in case any predicate cares.
    x_at(&mut b, &[(0, 0)]);
    let state = OrderingState::new();
    let killer = KillerSlot::default();
    let completion = mv(10, 10);
    let stone1_defense = [completion];
    let c = ctx(
        &b,
        Player::X,
        None,
        &killer,
        &state.history,
        &stone1_defense,
    );
    let s0_creator = mv(3, 5);
    let mut moves = list(&[s0_creator, completion]);
    order_moves(&mut moves, &c);
    assert_eq!(
        moves[0], completion,
        "stone-1 S0 completion (bucket 7) must beat creates-S0 (bucket 6)",
    );
}

// ────────────────────────────────────────────────────────────────────────
// 5. Blocks-opp-S0 outranks creates-S1
// ────────────────────────────────────────────────────────────────────────

#[test]
fn blocks_opp_s0_outranks_creates_s1() {
    let mut b = Board::new();
    // O closed-4 along q-axis: O at (0..3, 0), X cap at (-1, 0).
    o_at(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    x_at(&mut b, &[(-1, 0)]);
    // X open-2 on r=5; playing (2,5) extends to 3-run (creates_s1).
    x_at(&mut b, &[(0, 5), (1, 5)]);
    let state = OrderingState::new();
    let killer = KillerSlot::default();
    let c = ctx(&b, Player::X, None, &killer, &state.history, &[]);
    let block = mv(4, 0);
    let s1_creator = mv(2, 5);
    let mut moves = list(&[s1_creator, block]);
    order_moves(&mut moves, &c);
    assert_eq!(
        moves[0], block,
        "blocks-opp-S0 (bucket 5) must beat creates-S1 (bucket 4)",
    );
}

// ────────────────────────────────────────────────────────────────────────
// 6. Killer placement: beats history, loses to bucket-4+
// ────────────────────────────────────────────────────────────────────────

#[test]
fn killer_beats_history_loses_to_s1() {
    let mut b = Board::new();
    x_at(&mut b, &[(0, 5), (1, 5)]); // open-2; (2,5) creates_s1.
    x_at(&mut b, &[(0, 0)]); // anchor
    let mut state = OrderingState::new();
    let killer_cell = mv(20, 20);
    let history_cell = mv(30, 30);
    let s1_cell = mv(2, 5);
    let mut killers = KillerSlot::default();
    killers.push(killer_cell);
    // Seed a small history bump for history_cell so it isn't accidentally
    // tied with killer_cell's history.
    state.history.insert((history_cell, Player::X), 42);
    let c = ctx(&b, Player::X, None, &killers, &state.history, &[]);
    let mut moves = list(&[history_cell, killer_cell, s1_cell]);
    order_moves(&mut moves, &c);
    assert_eq!(moves[0], s1_cell, "creates_s1 (bucket 4) wins");
    assert_eq!(moves[1], killer_cell, "killer (bucket 3) beats history");
    assert_eq!(moves[2], history_cell, "history (bucket 1) last");
}

// ────────────────────────────────────────────────────────────────────────
// 7. History tie-break among bucket-1 moves
// ────────────────────────────────────────────────────────────────────────

#[test]
fn history_tiebreak_orders_bucket1() {
    let mut b = Board::new();
    x_at(&mut b, &[(0, 0)]);
    let mut state = OrderingState::new();
    let c_high = mv(20, 20);
    let c_mid = mv(22, 22);
    let c_low = mv(24, 24);
    state.history.insert((c_high, Player::X), 1000);
    state.history.insert((c_mid, Player::X), 500);
    state.history.insert((c_low, Player::X), 10);
    let killer = KillerSlot::default();
    let c = ctx(&b, Player::X, None, &killer, &state.history, &[]);
    let mut moves = list(&[c_low, c_mid, c_high]);
    order_moves(&mut moves, &c);
    assert_eq!(moves[0], c_high);
    assert_eq!(moves[1], c_mid);
    assert_eq!(moves[2], c_low);
}

// ────────────────────────────────────────────────────────────────────────
// 8. MOVE_GEN_CAP truncation
// ────────────────────────────────────────────────────────────────────────

#[test]
fn truncates_to_move_gen_cap() {
    let mut b = Board::new();
    x_at(&mut b, &[(0, 0)]);
    let state = OrderingState::new();
    let killer = KillerSlot::default();
    let c = ctx(&b, Player::X, None, &killer, &state.history, &[]);
    let mut moves: MoveList = SmallVec::new();
    // 50 far-apart cells with no tactical content — all bucket-1.
    for i in 0..50i16 {
        moves.push(mv(40 + i, 40));
    }
    order_moves(&mut moves, &c);
    assert_eq!(moves.len(), MOVE_GEN_CAP);
}

// ────────────────────────────────────────────────────────────────────────
// 9. History decay halves (integer floor)
// ────────────────────────────────────────────────────────────────────────

#[test]
fn decay_history_halves_each_entry() {
    let mut state = OrderingState::new();
    // record_cutoff(depth=3) adds 9. Two cutoffs at depth 3 = 18.
    state.record_cutoff(0, mv(0, 0), Player::X, 3);
    state.record_cutoff(0, mv(0, 0), Player::X, 3);
    state.record_cutoff(0, mv(1, 0), Player::X, 2); // +4
    state.record_cutoff(0, mv(2, 0), Player::O, 5); // +25

    let before: Vec<((Coord, Player), u32)> = state.history.iter().map(|(&k, &v)| (k, v)).collect();
    assert!(!before.is_empty());

    state.decay_history();

    for ((coord, player), old) in before {
        let new = state.history.get(&(coord, player)).copied().unwrap();
        assert_eq!(new, old / 2, "{coord:?} {player:?}: {old} -> {new}");
    }
}

// ────────────────────────────────────────────────────────────────────────
// 10. Killer dedup
// ────────────────────────────────────────────────────────────────────────

#[test]
fn killer_dedup_preserves_first_slot() {
    let mut k = KillerSlot::default();
    let c = mv(1, 1);
    let d = mv(2, 2);
    k.push(c);
    k.push(c); // dedup, no change
    assert_eq!(k.slots(), &[Some(c), None]);
    k.push(d);
    assert_eq!(k.slots(), &[Some(d), Some(c)]);
    // Pushing c again should dedup against slot 1 (not bubble).
    k.push(c);
    assert_eq!(k.slots(), &[Some(d), Some(c)]);
}

// ────────────────────────────────────────────────────────────────────────
// 11. History saturates at HISTORY_CUTOFF_MAX
// ────────────────────────────────────────────────────────────────────────

#[test]
fn history_saturates_at_cutoff_max() {
    let mut state = OrderingState::new();
    let m = mv(7, 7);
    // Pre-seed close to the cap then push it over.
    state.history.insert((m, Player::X), HISTORY_CUTOFF_MAX - 5);
    state.record_cutoff(0, m, Player::X, 100); // adds 10_000
    assert_eq!(
        state.history.get(&(m, Player::X)).copied(),
        Some(HISTORY_CUTOFF_MAX),
    );
}
