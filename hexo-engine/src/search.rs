#![allow(clippy::must_use_candidate)]

use crate::board::Board;
use crate::config;
use crate::coords::Coord;

#[derive(Copy, Clone, Debug)]
pub struct SearchConfig {
    pub max_depth: i8,
    pub time_ms: Option<u64>,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            max_depth: i8::try_from(config::DEFAULT_MAX_DEPTH)
                .expect("DEFAULT_MAX_DEPTH must fit in i8"),
            time_ms: Some(config::DEFAULT_TIME_MS),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct SearchResult {
    pub best_move: Coord,
    pub score: i32,
    pub depth: i8,
}

pub fn search(_board: &mut Board, _cfg: SearchConfig) -> SearchResult {
    todo!()
}
