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
//! Micro-benchmarks for [`hexo_engine::eval`].
//!
//! Layer-isolated benches call `#[doc(hidden)] pub fn bench_layer*` shims
//! exposed in `eval.rs` so we can time each layer independently without
//! re-running the others.

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use hexo_engine::board::Player;
use hexo_engine::coords::ORIGIN;
use hexo_engine::eval::{bench_layer1_window_scan, bench_layer2_shapes, bench_layer3_fork_bonus};

mod common;
use common::positions::FIXTURES;

fn bench_cached_eval_cold(c: &mut Criterion) {
    let mut group = c.benchmark_group("eval::cached_eval_cold");
    for fx in FIXTURES {
        group.bench_function(fx.name, |b| {
            b.iter_batched_ref(
                || {
                    // Force a fresh cache by placing+undoing a candidate.
                    let mut board = (fx.build)();
                    let target = board.candidates().next().unwrap_or(ORIGIN);
                    if board.place(target).is_ok() {
                        let _ = board.undo();
                    }
                    board
                },
                |board| {
                    black_box(board.cached_eval());
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_cached_eval_warm(c: &mut Criterion) {
    let mut group = c.benchmark_group("eval::cached_eval_warm");
    for fx in FIXTURES {
        let board = (fx.build)();
        let _prime = board.cached_eval();
        group.bench_function(fx.name, |b| {
            b.iter(|| black_box(board.cached_eval()));
        });
    }
    group.finish();
}

fn bench_layer1(c: &mut Criterion) {
    let mut group = c.benchmark_group("eval::layer1_window_scan");
    for fx in FIXTURES {
        let board = (fx.build)();
        group.bench_function(fx.name, |b| {
            b.iter(|| black_box(bench_layer1_window_scan(&board)));
        });
    }
    group.finish();
}

fn bench_layer2(c: &mut Criterion) {
    let mut group = c.benchmark_group("eval::layer2_shapes");
    for fx in FIXTURES {
        let board = (fx.build)();
        let _ = board.threats(Player::X);
        let _ = board.threats(Player::O);
        group.bench_function(fx.name, |b| {
            b.iter(|| black_box(bench_layer2_shapes(&board)));
        });
    }
    group.finish();
}

fn bench_layer3(c: &mut Criterion) {
    let mut group = c.benchmark_group("eval::layer3_fork_bonus");
    for fx in FIXTURES {
        let board = (fx.build)();
        group.bench_function(fx.name, |b| {
            b.iter(|| {
                let x = bench_layer3_fork_bonus(&board, Player::X);
                let o = bench_layer3_fork_bonus(&board, Player::O);
                black_box((x, o))
            });
        });
    }
    group.finish();
}


criterion_group!(
    benches,
    bench_cached_eval_cold,
    bench_cached_eval_warm,
    bench_layer1,
    bench_layer2,
    bench_layer3
);
criterion_main!(benches);
