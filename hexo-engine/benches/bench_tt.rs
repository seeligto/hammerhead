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
//! Micro-benchmarks for [`hexo_engine::tt::TranspositionTable`].

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use hexo_engine::coords::ORIGIN;
use hexo_engine::tt::{TTFlag, TranspositionTable};

mod common;
use common::positions::FIXTURES;

fn populate(tt: &mut TranspositionTable, hashes: &[u128]) {
    for (i, &h) in hashes.iter().enumerate() {
        let depth = (i % 16) as i8;
        let score = i as i32;
        tt.store(h, depth, score, TTFlag::Exact, ORIGIN);
    }
}

fn fixture_hashes(seed: u128, n: usize) -> Vec<u128> {
    let mut out = Vec::with_capacity(n);
    let mut s = seed;
    for _ in 0..n {
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        out.push(s);
    }
    out
}

fn bench_probe(c: &mut Criterion) {
    let mut group = c.benchmark_group("tt::probe");
    for fx in FIXTURES {
        let board = (fx.build)();
        let mut tt = TranspositionTable::new(4);
        let stored: Vec<u128> = fixture_hashes(board.hash(), 256);
        populate(&mut tt, &stored);
        let misses: Vec<u128> = fixture_hashes(board.hash() ^ 0xDEAD_BEEF, 256);
        group.bench_function(format!("hit/{}", fx.name), |b| {
            b.iter(|| {
                for h in &stored {
                    black_box(tt.probe(*h));
                }
            });
        });
        group.bench_function(format!("miss/{}", fx.name), |b| {
            b.iter(|| {
                for h in &misses {
                    black_box(tt.probe(*h));
                }
            });
        });
    }
    group.finish();
}

fn bench_store(c: &mut Criterion) {
    let mut group = c.benchmark_group("tt::store");
    for fx in FIXTURES {
        let board = (fx.build)();
        let hashes: Vec<u128> = fixture_hashes(board.hash(), 256);
        group.bench_function(format!("depth_preferred/{}", fx.name), |b| {
            b.iter_batched_ref(
                || TranspositionTable::new(4),
                |tt| {
                    for (i, &h) in hashes.iter().enumerate() {
                        let depth = ((i % 32) + 1) as i8;
                        tt.store(h, depth, i as i32, TTFlag::Exact, ORIGIN);
                    }
                },
                BatchSize::SmallInput,
            );
        });
        group.bench_function(format!("always_replace/{}", fx.name), |b| {
            b.iter_batched_ref(
                || {
                    let mut tt = TranspositionTable::new(4);
                    // Prime with deep entries so subsequent shallow stores
                    // fall through to the always-replace slot.
                    for (i, &h) in hashes.iter().enumerate() {
                        tt.store(h, 64, i as i32, TTFlag::Exact, ORIGIN);
                    }
                    tt
                },
                |tt| {
                    for (i, &h) in hashes.iter().enumerate() {
                        tt.store(h, 1, i as i32, TTFlag::Exact, ORIGIN);
                    }
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}


criterion_group!(benches, bench_probe, bench_store);
criterion_main!(benches);
