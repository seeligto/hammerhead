//! Production NNUE leaf-eval integration tests.
//!
//! Guards the productionised net: it loads from the codegen'd config, is
//! installed by default, the mate / fork / terminal logic still dominates,
//! and the shipped int16-quant path tracks the float path. The
//! `incremental == full-recompute` accumulator regression lives in the
//! `nnue` unit tests and `board::acc_tests`.

use hammerhead_engine_core::board::Player;
use hammerhead_engine_core::config::{MATE_SCORE, NNUE_ENABLED};
use hammerhead_engine_core::coords::Coord;
use hammerhead_engine_core::engine::Engine;
use hammerhead_engine_core::nnue::{features_full, production_net, FeatureKind};

/// A non-terminal midgame, X-to-move parity irrelevant for these checks.
const MIDGAME: &[(i16, i16)] = &[
    (0, 0),
    (1, 0),
    (0, 1),
    (-1, 1),
    (2, 0),
    (1, -1),
    (0, 2),
    (-1, 0),
];

fn play_alternating(e: &mut Engine, moves: &[(i16, i16)]) {
    for (i, &(q, r)) in moves.iter().enumerate() {
        let p = if i % 2 == 0 { Player::X } else { Player::O };
        e.board.place_for_test(Coord::new(q, r), p);
    }
}

/// The committed net loads from codegen'd config with the expected shape,
/// and (hexo.toml `quantize = true`) ships an int16 mirror.
#[test]
fn production_net_loads_with_quant() {
    let net = production_net();
    assert_eq!(net.kind, FeatureKind::PerAxis);
    assert_eq!(net.nfeat, 32);
    assert!(
        net.quant.is_some(),
        "quantize = true in hexo.toml -> int16 mirror expected"
    );
    assert!((net.out_scale - 600.0).abs() < 1e-3, "out_scale = {}", net.out_scale);
}

/// The net replaces the hand-built positional eval: installing it yields a
/// different score than the `None` fallback, and stays below the mate band.
#[test]
fn nnue_replaces_handbuilt_eval() {
    let mut e = Engine::new(16);
    play_alternating(&mut e, MIDGAME);

    e.set_nnue(Some(production_net()));
    let with_net = e.board.cached_eval();
    e.set_nnue(None); // runtime override -> hand-built fallback
    let hand_built = e.board.cached_eval();

    assert_ne!(
        with_net, hand_built,
        "net eval should differ from the hand-built positional eval"
    );
    // Net output stays strictly below the mate band so terminal logic wins.
    assert!(
        with_net.abs() < MATE_SCORE - 1000,
        "net eval {with_net} must stay below the mate band"
    );
}

/// The default engine honours `config::NNUE_ENABLED`: with the net enabled
/// (shipped default) a fresh engine already evaluates via the net, not the
/// hand-built eval. Encodes the flag without a constant assertion.
#[test]
fn default_engine_honours_config_flag() {
    let mut def = Engine::new(16);
    play_alternating(&mut def, MIDGAME);
    let default_eval = def.board.cached_eval();

    let mut probe = Engine::new(16);
    play_alternating(&mut probe, MIDGAME);
    probe.set_nnue(Some(production_net()));
    let net_eval = probe.board.cached_eval();
    probe.set_nnue(None);
    let hand_eval = probe.board.cached_eval();

    // The two paths must actually differ, else the check below is vacuous.
    assert_ne!(net_eval, hand_eval);
    let expected = if NNUE_ENABLED { net_eval } else { hand_eval };
    assert_eq!(
        default_eval, expected,
        "default engine must evaluate via the config-selected eval path"
    );
}

/// Mate / terminal logic dominates the net: a won position returns the mate
/// band even with the net installed (the net is clamped below it, so a
/// mate-band score can only come from the terminal short-circuit).
#[test]
fn mate_dominates_nnue() {
    let mut e = Engine::new(16); // NNUE on by default
                                 // X completes a 6-in-row on the q-axis; O scattered, non-blocking.
    e.board.place_for_test(Coord::new(0, 0), Player::X);
    e.board.place_for_test(Coord::new(0, 5), Player::O);
    e.board.place_for_test(Coord::new(1, 0), Player::X);
    e.board.place_for_test(Coord::new(0, 6), Player::O);
    e.board.place_for_test(Coord::new(2, 0), Player::X);
    e.board.place_for_test(Coord::new(0, 7), Player::O);
    e.board.place_for_test(Coord::new(3, 0), Player::X);
    e.board.place_for_test(Coord::new(0, 8), Player::O);
    e.board.place_for_test(Coord::new(4, 0), Player::X);
    e.board.place_for_test(Coord::new(0, 9), Player::O);
    e.board.place_for_test(Coord::new(5, 0), Player::X); // 6-in-row -> X wins

    assert_eq!(e.board.winner(), Some(Player::X), "expected X win");
    let score = e.board.cached_eval();
    assert!(
        score >= MATE_SCORE - 1000,
        "won position must score in the mate band (got {score}); the net is \
         clamped below this, so the terminal path dominated"
    );
}

/// The shipped int16-quant path tracks the float path on real board features
/// — validates that the committed weights quantise cleanly end-to-end.
#[test]
fn production_net_quant_tracks_float() {
    let net = production_net();
    assert!(net.quant.is_some());

    let cells: Vec<Coord> = MIDGAME.iter().map(|&(q, r)| Coord::new(q, r)).collect();
    let players: Vec<Player> = (0..cells.len())
        .map(|i| if i % 2 == 0 { Player::X } else { Player::O })
        .collect();

    for stm in [Player::X, Player::O] {
        let x = features_full(&cells, &players, stm, FeatureKind::PerAxis);
        let lf = net.forward_logit(&x);
        let lq = net.forward_logit_q(&x);
        assert!(
            (lf - lq).abs() < 0.05,
            "quant logit {lq} drifts from float {lf} (stm {stm:?})"
        );
    }
}
