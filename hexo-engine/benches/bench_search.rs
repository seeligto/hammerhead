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
//!
//! The `Engine` (and its 64 MB transposition table) is allocated **once** per
//! `bench_function` invocation and reused across criterion iterations via
//! `Engine::reset` + `Engine::clear_tt`. Constructing a fresh `Engine` per
//! iteration dominated the Phase 12 flamegraph with `from_elem` /
//! `unmap_region` / kernel zero-fill frames; reusing the allocation removes
//! that bench-setup artifact so future flamegraphs surface real search work.
//! Setup (reset + replay) is excluded from the measured time via
//! `Bencher::iter_custom`.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::time::{Duration, Instant};
use hexo_engine_core::search::Engine;

mod common;
use common::positions::FIXTURES;

fn bench_search_root(c: &mut Criterion) {
    let fx = FIXTURES
        .iter()
        .find(|f| f.name == "midgame_12")
        .expect("midgame_12 fixture must exist");
    let depths: [i8; 3] = [2, 4, 6];
    // Pre-compute the fixture's move list once. The Phase 13 bench
    // hoist amortized `Engine::new` across iterations but still
    // rebuilt a full `Board` (with its `FxHashSet`s and `Vec`s) every
    // iteration via `(fx.build)()`; the kernel-side `kernel_init_pages`
    // / `unmap_region` frames in the Phase 14 mid-phase flamegraph all
    // traced back to that per-iter Board churn. With the history
    // hoisted, the inner loop is allocation-free for setup.
    let history: Vec<_> = (fx.build)().history().to_vec();
    for &d in &depths {
        let mut group = c.benchmark_group(format!("search::search_root(depth={d})"));
        group.sample_size(10);
        let mut e = Engine::new(64);
        group.bench_function(fx.name, |b| {
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    // Reset only the board + TT. `OrderingState` (killers
                    // + history) is intentionally retained across
                    // iterations: it accumulates the same way it does
                    // between consecutive `best_move` calls in a real
                    // game, so the bench reflects warm-start search
                    // ordering rather than a pure cold start. Add
                    // `e.ordering = OrderingState::new()` here if a
                    // cold-start measurement is wanted.
                    e.reset();
                    e.clear_tt();
                    for c in &history {
                        e.board.place_for_test(
                            *c,
                            hexo_engine_core::board::player_at_ply(e.board.ply()),
                        );
                    }
                    let start = Instant::now();
                    let r = e.best_move(None, Some(d));
                    total += start.elapsed();
                    black_box((r.nodes, r.depth_reached, r.best_move));
                }
                total
            });
        });
        group.finish();
    }
}


criterion_group!(benches, bench_search_root);
criterion_main!(benches);
