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
    clippy::too_many_lines,
    clippy::must_use_candidate
)]
//! Shared bench fixture library.
//!
//! `build.rs` reads `benches/fixtures/positions.json` and codegens one
//! `build_<name>() -> Board` per fixture plus the [`FIXTURES`] slice into
//! `$OUT_DIR/fixtures_generated.rs`. Same JSON drives Python fixtures.

use hammerhead_engine_core::board::{Board, player_at_ply};
use hammerhead_engine_core::coords::Coord;

/// Named bench position.
pub struct Fixture {
    pub name: &'static str,
    pub build: fn() -> Board,
}

include!(concat!(env!("OUT_DIR"), "/fixtures_generated.rs"));
