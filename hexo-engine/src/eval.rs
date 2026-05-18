// Eval scores and weights are defined in hexo.toml and exposed via crate::config.
// Use crate::config::MATE_SCORE, OPEN_5_SCORE, etc.

use crate::board::Board;

pub fn eval(_board: &Board) -> i32 {
    todo!()
}
