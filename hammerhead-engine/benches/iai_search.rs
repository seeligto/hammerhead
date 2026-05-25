// Bench tooling — pedantic style lints add noise without value.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::missing_panics_doc
)]
//! Deterministic instruction-count benchmark over canonical fixtures.
//!
//! Phase 26.5 / 28F-2 sweeps were noise-bound at 200 g × 500 ms; this
//! gate resolves sub-1 % changes in seconds. Counts instructions /
//! cache misses / branch mispredictions under valgrind callgrind.
//!
//! Run via `cargo bench --bench iai_search` (requires valgrind).
//! Two consecutive runs must agree to ≤ 50 instructions per bench —
//! larger drift indicates host-state noise, not a code change.
//!
//! See `specs/SPEC_BENCHMARKS.md` § "iai-callgrind — deterministic gate".

use std::hint::black_box;

use hammerhead_engine_core::board::{Player, player_at_ply};
use hammerhead_engine_core::engine::Engine;
use iai_callgrind::{library_benchmark, library_benchmark_group, main};

mod common;
use common::positions::FIXTURES;

/// Build an engine with the named fixture's history replayed onto the
/// board. Runs OUTSIDE the measurement window — iai-callgrind's
/// `setup` parameter attributes its event counts to setup, not the
/// benchmark.
fn setup_engine(fixture_name: &str) -> Engine {
    let fx = FIXTURES
        .iter()
        .find(|f| f.name == fixture_name)
        .unwrap_or_else(|| panic!("fixture {fixture_name} not found"));
    let target = (fx.build)();
    let history: Vec<_> = target.history().to_vec();

    let mut e = Engine::new(64);
    for c in &history {
        let p: Player = player_at_ply(e.board.ply());
        e.board.place_for_test(*c, p);
    }
    e
}

#[library_benchmark(setup = setup_engine)]
#[bench::midgame_12("midgame_12")]
#[bench::midgame_30("midgame_30")]
fn bench_search_d6(mut engine: Engine) {
    let r = engine.best_move(None, Some(6));
    black_box((r.nodes, r.depth_reached, r.best_move));
}

library_benchmark_group!(name = search; benchmarks = bench_search_d6);
main!(library_benchmark_groups = search);
