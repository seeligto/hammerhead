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
//! Micro-benchmarks for [`hammerhead_engine_core::threats`].

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use hammerhead_engine_core::board::Player;
use hammerhead_engine_core::threats;

mod common;
use common::positions::FIXTURES;

fn bench_compute_full(c: &mut Criterion) {
    let mut group = c.benchmark_group("threats::compute_full");
    for fx in FIXTURES {
        let board = (fx.build)();
        group.bench_function(fx.name, |b| {
            b.iter(|| {
                let tx = threats::compute(&board, Player::X);
                let to = threats::compute(&board, Player::O);
                black_box((tx.counts.open_4, to.counts.open_4))
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
        let tx = threats::compute(&board, Player::X);
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
    bench_defense_cells_read
);
criterion_main!(benches);
