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
//! Micro-benchmarks for [`hammerhead_engine_core::axis_bitmap`].

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use hammerhead_engine_core::axis_bitmap::Axis;
use hammerhead_engine_core::board::Player;
use hammerhead_engine_core::coords::Coord;

mod common;
use common::positions::FIXTURES;

const PROBE: Coord = Coord { q: 1, r: 0 };

fn bench_set_clear(c: &mut Criterion) {
    let mut group = c.benchmark_group("axis_bitmap::set_clear");
    for fx in FIXTURES {
        group.bench_function(fx.name, |b| {
            b.iter_batched_ref(
                || (fx.build)(),
                |board| {
                    let target = board.candidates().next().unwrap_or(PROBE);
                    let _ = black_box(board.place(target));
                    let _ = black_box(board.undo());
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_window6(c: &mut Criterion) {
    let mut group = c.benchmark_group("axis_bitmap::window6");
    for fx in FIXTURES {
        let board = (fx.build)();
        // Pre-collect per-(axis, player) line ids so the timed body
        // doesn't allocate a Vec on every iteration.
        let mut probes: Vec<(Axis, Player, Vec<i16>)> = Vec::with_capacity(6);
        for axis in Axis::all() {
            for player in [Player::X, Player::O] {
                probes.push((
                    axis,
                    player,
                    board.axes().line_ids(axis, player).collect(),
                ));
            }
        }
        group.bench_function(fx.name, |b| {
            b.iter(|| {
                let mut sum = 0u32;
                for (axis, player, line_ids) in &probes {
                    for &line_id in line_ids {
                        if let Some(line) = board.axes().line(*axis, *player, line_id)
                            && let Some((lo, hi)) = line.populated_range()
                        {
                            for pos in lo..=hi {
                                sum += u32::from(
                                    board.axes().window6(*axis, line_id, pos, *player),
                                );
                            }
                        }
                    }
                }
                black_box(sum)
            });
        });
    }
    group.finish();
}

fn bench_run_through(c: &mut Criterion) {
    let mut group = c.benchmark_group("axis_bitmap::run_through");
    for fx in FIXTURES {
        let board = (fx.build)();
        let probe: Vec<Coord> = board.pieces().take(8).map(|(c, _)| c).collect();
        group.bench_function(fx.name, |b| {
            b.iter(|| {
                let mut sum = 0u32;
                for c in &probe {
                    for axis in Axis::all() {
                        for player in [Player::X, Player::O] {
                            sum += u32::from(board.axes().run_length_through(*c, axis, player));
                        }
                    }
                }
                black_box(sum)
            });
        });
    }
    group.finish();
}

fn bench_populated_range(c: &mut Criterion) {
    let mut group = c.benchmark_group("axis_bitmap::populated_range");
    for fx in FIXTURES {
        let board = (fx.build)();
        let mut probes: Vec<(Axis, Player, Vec<i16>)> = Vec::with_capacity(6);
        for axis in Axis::all() {
            for player in [Player::X, Player::O] {
                probes.push((
                    axis,
                    player,
                    board.axes().line_ids(axis, player).collect(),
                ));
            }
        }
        group.bench_function(fx.name, |b| {
            b.iter(|| {
                let mut sum = 0i32;
                for (axis, player, line_ids) in &probes {
                    for &line_id in line_ids {
                        if let Some(line) = board.axes().line(*axis, *player, line_id)
                            && let Some((lo, hi)) = line.populated_range()
                        {
                            sum += i32::from(hi - lo);
                        }
                    }
                }
                black_box(sum)
            });
        });
    }
    group.finish();
}


criterion_group!(
    benches,
    bench_set_clear,
    bench_window6,
    bench_run_through,
    bench_populated_range
);
criterion_main!(benches);
