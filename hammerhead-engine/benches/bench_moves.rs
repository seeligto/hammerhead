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
//! Micro-benchmarks for [`hammerhead_engine_core::moves::generate`].

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use hammerhead_engine_core::moves;

mod common;
use common::positions::FIXTURES;

/// Bench `generate` at three radii: inner default (2), extended (4),
/// and the legality cap (8) — covers the three internal dispatch paths.
fn bench_generate(c: &mut Criterion) {
    let radii: [i16; 3] = [2, 4, 8];
    for &r in &radii {
        let group_name = format!("moves::generate(r={r})");
        let mut group = c.benchmark_group(group_name);
        for fx in FIXTURES {
            let board = (fx.build)();
            group.bench_function(fx.name, |b| {
                b.iter(|| {
                    let list = moves::generate(&board, r);
                    black_box(list.len())
                });
            });
        }
        group.finish();
    }
}


criterion_group!(benches, bench_generate);
criterion_main!(benches);
