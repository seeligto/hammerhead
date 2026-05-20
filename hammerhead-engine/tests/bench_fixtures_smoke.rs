//! Smoke test: every named fixture in `benches/fixtures/positions.json`
//! must build via the codegen'd `build_<name>()` without panicking.
//!
//! Mirrors the `common::positions` module used by the criterion benches —
//! same `include!` of `$OUT_DIR/fixtures_generated.rs`, so this test
//! catches a broken fixture before any bench tries to use it.

#![allow(clippy::must_use_candidate, clippy::let_and_return)]

use hammerhead_engine_core::board::{Board, player_at_ply};
use hammerhead_engine_core::coords::Coord;

pub struct Fixture {
    pub name: &'static str,
    pub build: fn() -> Board,
}

include!(concat!(env!("OUT_DIR"), "/fixtures_generated.rs"));

#[test]
fn every_fixture_builds() {
    for fx in FIXTURES {
        let board = (fx.build)();
        // Sanity: empty fixture has ply 0, every other fixture > 0.
        if fx.name == "empty" {
            assert_eq!(board.ply(), 0, "empty fixture must have ply 0");
        } else {
            assert!(
                board.ply() > 0,
                "fixture {} expected ply > 0, got {}",
                fx.name,
                board.ply(),
            );
        }
    }
}

#[test]
fn fixtures_table_nonempty() {
    assert!(!FIXTURES.is_empty(), "FIXTURES table must not be empty");
}
