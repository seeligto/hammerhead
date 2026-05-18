pub mod axis_bitmap;
pub mod board;
pub mod config;
pub mod coords;
pub mod eval;
pub mod moves;
pub mod ordering;
pub mod pybind;
pub mod search;
pub mod threats;
pub mod tt;
pub mod win;
pub mod zobrist;

pub use axis_bitmap::{Axis, AxisBitmaps};
pub use board::{Board, BoardError, Player, halfmove_at_ply, player_at_ply};
pub use config::*;
pub use coords::{
    AXES, AXIS_Q, AXIS_R, AXIS_S, Coord, ORIGIN, RANGE_OFFSET_COUNT, RANGE_OFFSETS,
    for_each_in_range, hex_distance, within_range,
};
pub use eval::{eval, is_mate_for};
pub use moves::{MOVE_GEN_CAP_INLINE, MoveList, generate};
pub use ordering::order_moves;
pub use search::{SearchConfig, SearchResult, search};
pub use threats::{
    ThreatCounts, ThreatInstance, ThreatKind, ThreatSet, compute as compute_threats,
    single_cell_blocks_all,
};
pub use tt::{TTEntry, TTFlag, TranspositionTable};
pub use win::is_winning_move;
pub use zobrist::{Z_HALFMOVE, Z_TURN_X, ZobristTable};
