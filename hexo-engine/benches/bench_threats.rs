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
//! Micro-benchmarks for [`hexo_engine::threats`].

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use hexo_engine::board::Player;
use hexo_engine::threats;

mod common;
use common::positions::FIXTURES;

fn bench_compute_full(c: &mut Criterion) {
    let mut group = c.benchmark_group("threats::compute_full");
    for fx in FIXTURES {
        let board = (fx.build)();
        group.bench_function(fx.name, |b| {
            b.iter(|| {
                let tx = threats::compute(&board, Player::X, None, None);
                let to = threats::compute(&board, Player::O, None, None);
                black_box((tx.counts.open_4, to.counts.open_4))
            });
        });
    }
    group.finish();
}

fn bench_single_cell_blocks_all(c: &mut Criterion) {
    let mut group = c.benchmark_group("threats::single_cell_blocks_all");
    for fx in FIXTURES {
        let board = (fx.build)();
        let tx = threats::compute(&board, Player::X, None, None);
        let to = threats::compute(&board, Player::O, None, None);
        group.bench_function(fx.name, |b| {
            b.iter(|| {
                black_box(threats::single_cell_blocks_all(&tx.s0_instances))
                    | black_box(threats::single_cell_blocks_all(&to.s0_instances))
            });
        });
    }
    group.finish();
}

/// Defense-cells extraction: the `compute` output already carries
/// per-instance defense cells. This bench reads them as the search would.
fn bench_defense_cells_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("threats::defense_cells_read");
    for fx in FIXTURES {
        let board = (fx.build)();
        let tx = threats::compute(&board, Player::X, None, None);
        group.bench_function(fx.name, |b| {
            b.iter(|| {
                let mut n = 0usize;
                for inst in &tx.s0_instances {
                    n += inst.defense_cells.len();
                }
                black_box(n)
            });
        });
    }
    group.finish();
}


criterion_group!(
    benches,
    bench_compute_full,
    bench_single_cell_blocks_all,
    bench_defense_cells_read
);
criterion_main!(benches);
