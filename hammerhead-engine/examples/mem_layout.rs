// Diagnostic print binary — pedantic style lints add noise without value.
#![allow(clippy::too_many_lines, clippy::similar_names)]
//! Struct size / alignment audit for the Phase 24 hotspot investigation.
//!
//! Emits `size_of` + `align_of` for every struct on the search hot path,
//! then the computed heap footprint of the allocation-owning containers
//! (derived from the public `hexo.toml` config constants). Read-only —
//! not wired into `make`. Run manually:
//!
//! ```text
//! cargo run --release --example mem_layout
//! ```

use std::mem::{align_of, size_of};

use hammerhead_engine_core::axis_bitmap::LineBitmap;
use hammerhead_engine_core::proximity::{ProximityCounts, SparseCellSet};
use hammerhead_engine_core::threats::ThreatScratch;
use hammerhead_engine_core::{
    Axis, AxisBitmaps, Board, Coord, DEFAULT_TT_SIZE_MB, Engine, KillerSlot, MAX_PIECE_DISTANCE,
    MAX_PLY, OrderingState, Player, SearchConfig, SearchResult, TTEntry, TTFlag, ThreatCounts,
    ThreatInstance, ThreatKind, ThreatSet, TranspositionTable, ZOBRIST_WINDOW, ZobristTable,
};

fn row(name: &str, size: usize, align: usize) {
    println!("  {name:<24} size = {size:>9}   align = {align:>3}");
}

fn main() {
    println!("== stack sizes (size_of / align_of) ==");
    row("Coord", size_of::<Coord>(), align_of::<Coord>());
    row("Player", size_of::<Player>(), align_of::<Player>());
    row("Axis", size_of::<Axis>(), align_of::<Axis>());
    row(
        "Option<Coord>",
        size_of::<Option<Coord>>(),
        align_of::<Option<Coord>>(),
    );
    row("LineBitmap", size_of::<LineBitmap>(), align_of::<LineBitmap>());
    row(
        "Option<LineBitmap>",
        size_of::<Option<LineBitmap>>(),
        align_of::<Option<LineBitmap>>(),
    );
    row("TTFlag", size_of::<TTFlag>(), align_of::<TTFlag>());
    row("TTEntry", size_of::<TTEntry>(), align_of::<TTEntry>());
    row(
        "(TTEntry, TTEntry)",
        size_of::<(TTEntry, TTEntry)>(),
        align_of::<(TTEntry, TTEntry)>(),
    );
    row("ThreatKind", size_of::<ThreatKind>(), align_of::<ThreatKind>());
    row(
        "ThreatCounts",
        size_of::<ThreatCounts>(),
        align_of::<ThreatCounts>(),
    );
    row(
        "ThreatInstance",
        size_of::<ThreatInstance>(),
        align_of::<ThreatInstance>(),
    );
    row("ThreatSet", size_of::<ThreatSet>(), align_of::<ThreatSet>());
    row(
        "ThreatScratch",
        size_of::<ThreatScratch>(),
        align_of::<ThreatScratch>(),
    );
    row("KillerSlot", size_of::<KillerSlot>(), align_of::<KillerSlot>());
    row(
        "SearchConfig",
        size_of::<SearchConfig>(),
        align_of::<SearchConfig>(),
    );
    row(
        "SearchResult",
        size_of::<SearchResult>(),
        align_of::<SearchResult>(),
    );
    row("AxisBitmaps", size_of::<AxisBitmaps>(), align_of::<AxisBitmaps>());
    row(
        "ProximityCounts",
        size_of::<ProximityCounts>(),
        align_of::<ProximityCounts>(),
    );
    row(
        "SparseCellSet",
        size_of::<SparseCellSet>(),
        align_of::<SparseCellSet>(),
    );
    row("ZobristTable", size_of::<ZobristTable>(), align_of::<ZobristTable>());
    row(
        "OrderingState",
        size_of::<OrderingState>(),
        align_of::<OrderingState>(),
    );
    row(
        "TranspositionTable",
        size_of::<TranspositionTable>(),
        align_of::<TranspositionTable>(),
    );
    row("Board", size_of::<Board>(), align_of::<Board>());
    row("Engine", size_of::<Engine>(), align_of::<Engine>());

    println!();
    println!("== derived heap footprints (from hexo.toml constants) ==");
    let zw = usize::try_from(ZOBRIST_WINDOW).expect("ZOBRIST_WINDOW is positive");
    let mpd = usize::try_from(MAX_PIECE_DISTANCE).expect("MAX_PIECE_DISTANCE is positive");

    let line_id_range = 4 * zw + 1;
    let opt_lb = size_of::<Option<LineBitmap>>();
    let axisbitmaps_heap = 9 * line_id_range * opt_lb;
    println!("  ZOBRIST_WINDOW         = {zw}");
    println!("  MAX_PIECE_DISTANCE     = {mpd}");
    println!("  LINE_ID_RANGE          = {line_id_range}");
    println!(
        "  AxisBitmaps heap       = {axisbitmaps_heap} B  (9 arrays x {line_id_range} x {opt_lb} B)"
    );

    let prox_half = zw + mpd;
    let prox_range = 2 * prox_half + 1;
    let prox_field = prox_range * prox_range;
    println!("  PROX_FIELD_SIZE        = {prox_field}");
    println!("  SparseCellSet.slot     = {} B  (u32 x {prox_field})", prox_field * 4);
    println!(
        "  ProximityCounts heap   = {} B  (2 x u8 x {prox_field})",
        prox_field * 2
    );

    let zob_side = 2 * zw + 1;
    let zob_window = zob_side * zob_side * 2;
    println!(
        "  ZobristTable.window    = {} B  (u128 x {zob_window})",
        zob_window * 16
    );

    let killers_heap = MAX_PLY * size_of::<KillerSlot>();
    println!("  OrderingState.killers  = {killers_heap} B  (KillerSlot x {MAX_PLY})");

    let bucket = size_of::<(TTEntry, TTEntry)>();
    let raw_buckets = DEFAULT_TT_SIZE_MB * 1024 * 1024 / bucket;
    // Mirror `tt.rs::floor_pow2`: largest power of two <= raw_buckets.
    let pow2_buckets = if raw_buckets == 0 {
        1
    } else {
        1usize << (usize::BITS - 1 - raw_buckets.leading_zeros())
    };
    println!("  TT bucket pair         = {bucket} B");
    println!(
        "  TT @ {DEFAULT_TT_SIZE_MB} MB           = {pow2_buckets} buckets ({} B heap)",
        pow2_buckets * bucket
    );
}
