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
//! Micro-benchmarks for [`hammerhead_engine_core::board::Board`] — place, undo, round-trip.
//!
//! Run via `cargo bench --bench bench_board` (from `hammerhead-engine/`).

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use hammerhead_engine_core::board::Board;
use hammerhead_engine_core::coords::{Coord, ORIGIN};

mod common;
use common::positions::FIXTURES;

/// Pick a legal target cell for a fresh placement on `board`.
/// Falls back to `ORIGIN` on an empty board (the forced opening move).
fn pick_target(board: &Board) -> Coord {
    board.candidates().next().unwrap_or(ORIGIN)
}

fn bench_place(c: &mut Criterion) {
    let mut group = c.benchmark_group("board::place");
    for fx in FIXTURES {
        group.bench_function(fx.name, |b| {
            b.iter_batched(
                || {
                    let board = (fx.build)();
                    let target = pick_target(&board);
                    (board, target)
                },
                |(mut board, target)| {
                    let _ = black_box(board.place(target));
                    board
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_undo(c: &mut Criterion) {
    let mut group = c.benchmark_group("board::undo");
    for fx in FIXTURES {
        if (fx.build)().ply() == 0 {
            continue;
        }
        group.bench_function(fx.name, |b| {
            b.iter_batched(
                || (fx.build)(),
                |mut board| {
                    let _ = black_box(board.undo());
                    board
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_place_undo_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("board::place_undo_roundtrip");
    for fx in FIXTURES {
        group.bench_function(fx.name, |b| {
            b.iter_batched(
                || {
                    let board = (fx.build)();
                    let target = pick_target(&board);
                    (board, target)
                },
                |(mut board, target)| {
                    let _ = board.place(target);
                    let _ = black_box(board.undo());
                    board
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}


criterion_group!(benches, bench_place, bench_undo, bench_place_undo_roundtrip);
criterion_main!(benches);
