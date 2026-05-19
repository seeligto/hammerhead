// Bench tooling — pedantic style lints add noise without value.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::map_unwrap_or,
    clippy::let_and_return,
    clippy::needless_pass_by_value,
    clippy::missing_panics_doc,
    clippy::missing_errors_doc,
    clippy::redundant_closure_for_method_calls,
    clippy::manual_let_else,
    clippy::single_match_else,
    clippy::too_many_lines
)]
//! Micro-benchmarks for the iterative-deepening search driver.
//!
//! Runs `Engine::best_move(depth=N)` (no time cap) on `midgame_12` at three
//! depths. Records nodes via the returned [`SearchResult`].

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use hexo_engine_core::search::Engine;

mod common;
use common::positions::FIXTURES;

fn bench_search_root(c: &mut Criterion) {
    let fx = FIXTURES
        .iter()
        .find(|f| f.name == "midgame_12")
        .expect("midgame_12 fixture must exist");
    let depths: [i8; 3] = [2, 4, 6];
    for &d in &depths {
        let mut group = c.benchmark_group(format!("search::search_root(depth={d})"));
        group.sample_size(10);
        group.bench_function(fx.name, |b| {
            b.iter_batched_ref(
                || {
                    let mut e = Engine::new(64);
                    // Replay fixture into a real Engine. Cheap enough.
                    let template = (fx.build)();
                    for c in template.history() {
                        e.board.place_for_test(
                            *c,
                            hexo_engine_core::board::player_at_ply(e.board.ply()),
                        );
                    }
                    e
                },
                |e| {
                    let r = e.best_move(None, Some(d));
                    black_box((r.nodes, r.depth_reached, r.best_move))
                },
                BatchSize::PerIteration,
            );
        });
        group.finish();
    }
}


criterion_group!(benches, bench_search_root);
criterion_main!(benches);
