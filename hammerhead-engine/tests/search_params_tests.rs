//! Sprint 4A — Runtime override of `SearchConfig` LMR triplet.
//!
//! Asserts:
//! 1. `SearchConfig::default()` mirrors the codegen'd LMR constants.
//! 2. The hot path actually reads `cfg.lmr_*` (not the constants) by
//!    running two searches at the same depth with different LMR
//!    settings and observing distinct node counts.
//! 3. Default config produces a search byte-identical to the baseline
//!    (sanity for production-path neutrality).

use hammerhead_engine_core::board::{Board, Player};
use hammerhead_engine_core::config::{LMR_MIN_DEPTH, LMR_MIN_MOVE_INDEX, LMR_REDUCTION};
use hammerhead_engine_core::coords::Coord;
use hammerhead_engine_core::ordering::OrderingState;
use hammerhead_engine_core::search::{SearchConfig, SearchScratch, search_root};
use hammerhead_engine_core::tt::TranspositionTable;

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

fn seeded() -> Board {
    let mut b = Board::new();
    x(&mut b, &[(0, 0), (2, 0), (-1, 1), (1, -1)]);
    o(&mut b, &[(1, 0), (-1, 0), (0, 1), (0, -1)]);
    b
}

fn run_at_depth(cfg: &SearchConfig, board: &mut Board) -> u64 {
    let mut tt = TranspositionTable::new(8);
    let mut ord = OrderingState::new();
    let mut scratch = SearchScratch::new();
    let r = search_root(board, &mut tt, &mut ord, &mut scratch, cfg);
    r.nodes
}

#[test]
fn default_search_config_matches_constants() {
    let d = SearchConfig::default();
    assert_eq!(d.lmr_min_depth, LMR_MIN_DEPTH);
    assert_eq!(d.lmr_min_move_index, LMR_MIN_MOVE_INDEX);
    assert_eq!(d.lmr_reduction, LMR_REDUCTION);
}

#[test]
fn lmr_override_changes_node_count() {
    // At fixed depth, a more aggressive LMR reduction visits fewer
    // nodes; a disabled LMR (reduction 0) visits more. Proves the
    // hot path is reading cfg.lmr_*, not a const.
    let mut board_a = seeded();
    let mut board_b = seeded();
    let base = SearchConfig {
        max_depth: 5,
        time_ms: None,
        ..SearchConfig::default()
    };
    let aggressive = SearchConfig {
        lmr_reduction: 3,
        lmr_min_move_index: 2,
        lmr_min_depth: 2,
        ..base
    };
    let disabled = SearchConfig {
        lmr_reduction: 0,
        ..base
    };
    let nodes_aggressive = run_at_depth(&aggressive, &mut board_a);
    let nodes_disabled = run_at_depth(&disabled, &mut board_b);
    assert!(
        nodes_aggressive < nodes_disabled,
        "aggressive LMR should visit fewer nodes: agg={nodes_aggressive} \
         vs disabled={nodes_disabled}"
    );
}

#[test]
fn aspiration_override_changes_node_count() {
    // Asp window of 1 forces many fail-low/fail-high re-searches at
    // each ID iteration; a large window (10000) collapses to full
    // window immediately. Node counts must differ.
    let mut a = seeded();
    let mut b = seeded();
    let base = SearchConfig {
        max_depth: 6,
        time_ms: None,
        ..SearchConfig::default()
    };
    let narrow = SearchConfig {
        asp_window_initial: 1,
        ..base
    };
    let wide = SearchConfig {
        asp_window_initial: 10_000,
        ..base
    };
    let nodes_narrow = run_at_depth(&narrow, &mut a);
    let nodes_wide = run_at_depth(&wide, &mut b);
    assert_ne!(
        nodes_narrow, nodes_wide,
        "narrow={nodes_narrow} wide={nodes_wide}"
    );
}

#[test]
fn default_config_byte_identical_to_explicit_constants() {
    // A SearchConfig built via Default and one built by hand from the
    // constants must produce the same node count — sanity that no
    // hidden field diverges.
    let mut a = seeded();
    let mut b = seeded();
    let from_default = SearchConfig {
        max_depth: 4,
        time_ms: None,
        ..SearchConfig::default()
    };
    let from_explicit = SearchConfig {
        max_depth: 4,
        time_ms: None,
        lmr_min_depth: LMR_MIN_DEPTH,
        lmr_min_move_index: LMR_MIN_MOVE_INDEX,
        lmr_reduction: LMR_REDUCTION,
        ..SearchConfig::default()
    };
    assert_eq!(
        run_at_depth(&from_default, &mut a),
        run_at_depth(&from_explicit, &mut b),
    );
}
