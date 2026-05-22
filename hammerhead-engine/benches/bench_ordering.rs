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
//! Micro-benchmarks for [`hammerhead_engine_core::ordering`].

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use fxhash::FxHashMap;
use hammerhead_engine_core::board::{Board, Player};
use hammerhead_engine_core::coords::Coord;
use hammerhead_engine_core::moves;
use hammerhead_engine_core::ordering::{
    KillerSlot, OrderingContext, bench_bucket_value, order_moves,
};

mod common;
use common::positions::FIXTURES;

fn make_ctx<'a>(
    board: &'a Board,
    killers: &'a KillerSlot,
    history: &'a FxHashMap<(Coord, Player), u32>,
) -> OrderingContext<'a> {
    OrderingContext {
        board,
        side: board.to_move(),
        tt_move: None,
        killers,
        history,
        stone1_s0_defense: &[],
    }
}

fn bench_order_moves(c: &mut Criterion) {
    let mut group = c.benchmark_group("ordering::order_moves");
    let killers = KillerSlot::default();
    let history: FxHashMap<(Coord, Player), u32> = FxHashMap::default();

    for fx in FIXTURES {
        let board = (fx.build)();
        let mut template: Vec<Coord> = Vec::new();
        moves::generate(&board, 4, &mut template);
        group.bench_function(fx.name, |b| {
            b.iter_batched_ref(
                || template.clone(),
                |list| {
                    let ctx = make_ctx(&board, &killers, &history);
                    order_moves(list, &ctx);
                    black_box(list.len());
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_bucket(c: &mut Criterion) {
    let mut group = c.benchmark_group("ordering::bucket_value");
    let killers = KillerSlot::default();
    let history: FxHashMap<(Coord, Player), u32> = FxHashMap::default();
    for fx in FIXTURES {
        let board = (fx.build)();
        let mut candidates: Vec<Coord> = Vec::new();
        moves::generate(&board, 4, &mut candidates);
        group.bench_function(fx.name, |b| {
            b.iter(|| {
                let ctx = make_ctx(&board, &killers, &history);
                let mut sum = 0u32;
                for m in &candidates {
                    sum += u32::from(bench_bucket_value(&ctx, *m));
                }
                black_box(sum)
            });
        });
    }
    group.finish();
}


criterion_group!(benches, bench_order_moves, bench_bucket);
criterion_main!(benches);
