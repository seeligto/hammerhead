//! Phase 28B-1 — `EvalOverrides` integration tests.
//!
//! Asserts:
//! 1. `Default` mirrors live `crate::config::*` constants.
//! 2. Patching an override changes the relevant eval term.
//! 3. Setting `Default` overrides over a clean board is byte-identical
//!    to never calling the setter — the "byte-identical-default" gate.
//! 4. Setter persists across `reset`.

use hammerhead_engine_core::board::{Board, Player};
use hammerhead_engine_core::config::{
    CLOSED_4_SCORE, CLOSED_5_SCORE, CLOSED_EXTENSION_FACTOR, FORK_COVER2_BONUS, MATE_SCORE,
    OPEN_4_SCORE, OPEN_5_SCORE, OPEN_EXTENSION_FACTOR,
};
use hammerhead_engine_core::coords::Coord;
use hammerhead_engine_core::eval::eval;
use hammerhead_engine_core::eval_overrides::EvalOverrides;

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

#[test]
fn default_overrides_match_config_constants() {
    let d = EvalOverrides::default();
    assert_eq!(d.open_5, OPEN_5_SCORE);
    assert_eq!(d.closed_5, CLOSED_5_SCORE);
    assert_eq!(d.open_4, OPEN_4_SCORE);
    assert_eq!(d.closed_4, CLOSED_4_SCORE);
    assert_eq!(d.open_extension_factor, OPEN_EXTENSION_FACTOR);
    assert_eq!(d.closed_extension_factor, CLOSED_EXTENSION_FACTOR);
    assert_eq!(d.fork_cover2_bonus, FORK_COVER2_BONUS);
    // window_k_scores[6] must equal mate_score (codegen invariant).
    assert_eq!(d.window_k_scores[6], MATE_SCORE);
}

/// Building a non-trivial position and applying the *default* overrides
/// must leave the static eval unchanged — the byte-identical-default
/// gate that makes the override invisible when nothing was tuned.
#[test]
fn default_set_is_eval_byte_identical() {
    let mut a = Board::new();
    x(&mut a, &[(0, 0), (1, 0), (2, 0)]);
    o(&mut a, &[(0, 1), (1, 1)]);
    let baseline = eval(&a);

    let mut b = Board::new();
    x(&mut b, &[(0, 0), (1, 0), (2, 0)]);
    o(&mut b, &[(0, 1), (1, 1)]);
    b.set_eval_overrides(EvalOverrides::default());
    assert_eq!(eval(&b), baseline);
}

/// A change to `open_4` perturbs Layer-2 and the resulting eval. Test
/// also confirms unrelated terms (fork bonus, S0 weights) are unaffected
/// — round-trip back to defaults restores the original eval byte-for-byte.
#[test]
fn open_4_override_changes_eval_and_round_trips() {
    let mut b = Board::new();
    // Construct a position that materialises a Layer-2 S0 instance for X.
    // Use an open-4 along the Q axis.
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    let baseline = eval(&b);

    let ov = EvalOverrides {
        open_4: OPEN_4_SCORE + 1_000,
        ..EvalOverrides::default()
    };
    b.set_eval_overrides(ov);
    let tuned = eval(&b);
    assert_ne!(
        tuned, baseline,
        "open_4 override must perturb a position that contains open-4 threats"
    );

    // Round-trip: reverting to defaults restores the baseline.
    b.set_eval_overrides(EvalOverrides::default());
    assert_eq!(eval(&b), baseline);
}

/// Layer-1 inputs (`window_k_scores`, extension factors) feed the
/// 6561-entry `WINDOW_SCORE_8` table. Override → table rebuild →
/// Layer-1 sees different values → eval differs. Revert → baseline.
#[test]
fn window_k_override_changes_layer1_and_round_trips() {
    let mut b = Board::new();
    x(&mut b, &[(0, 0), (1, 0), (2, 0)]);
    o(&mut b, &[(-1, 1), (-2, 2)]);
    let baseline = eval(&b);

    // Bump the k=3 score (live midgame contributor) without touching
    // k=6 (which must remain == mate_score).
    let mut new_k = EvalOverrides::default().window_k_scores;
    new_k[3] += 1_000;
    let ov = EvalOverrides {
        window_k_scores: new_k,
        ..EvalOverrides::default()
    };
    b.set_eval_overrides(ov);
    let tuned = eval(&b);
    assert_ne!(tuned, baseline);

    b.set_eval_overrides(EvalOverrides::default());
    assert_eq!(eval(&b), baseline);
}

/// `fork_cover2_bonus` only fires on cover-2 fork positions; tweaking
/// it on a non-fork position must not change the eval (proves the
/// override is wired through Layer 3, not bleeding into other layers).
#[test]
fn fork_override_unrelated_position_is_neutral() {
    let mut b = Board::new();
    x(&mut b, &[(0, 0), (1, 0)]);
    o(&mut b, &[(0, 1)]);
    let baseline = eval(&b);

    let ov = EvalOverrides {
        fork_cover2_bonus: FORK_COVER2_BONUS + 999_999,
        ..EvalOverrides::default()
    };
    b.set_eval_overrides(ov);
    assert_eq!(eval(&b), baseline, "no cover-2 fork: bonus must be inert");
}

/// `set_eval_overrides` persists across `reset` (Phase 18 precedent).
#[test]
fn overrides_persist_across_reset() {
    let mut b = Board::new();
    let ov = EvalOverrides {
        open_5: OPEN_5_SCORE + 7,
        ..EvalOverrides::default()
    };
    b.set_eval_overrides(ov);
    b.reset();
    let after = b.eval_overrides();
    assert_eq!(after.open_5, OPEN_5_SCORE + 7);
}
