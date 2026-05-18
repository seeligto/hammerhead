use crate::coords::Coord;
use fxhash::{FxHashMap, FxHashSet};
use thiserror::Error;

#[repr(u8)]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum Player {
    X = 0,
    O = 1,
}

#[derive(Debug, Error)]
pub enum BoardError {
    #[error("illegal move")]
    Illegal,
    #[error("no move to undo")]
    EmptyHistory,
}

pub type BoardResult<T> = Result<T, BoardError>;

pub struct Board {
    pub(crate) pieces: FxHashMap<Coord, Player>,
    pub(crate) hash: u64,
    pub(crate) ply: u32,
    pub(crate) history: Vec<Coord>,
    pub(crate) candidate_cells: FxHashSet<Coord>,
}

impl Default for Board {
    fn default() -> Self {
        Self::new()
    }
}

impl Board {
    pub fn new() -> Self {
        Self {
            pieces: FxHashMap::default(),
            hash: 0,
            ply: 0,
            history: Vec::with_capacity(128),
            candidate_cells: FxHashSet::default(),
        }
    }

    pub fn place(&mut self, _c: Coord) -> BoardResult<()> {
        todo!()
    }

    pub fn undo(&mut self) -> BoardResult<()> {
        todo!()
    }

    pub fn to_move(&self) -> Player {
        todo!()
    }

    pub fn is_legal(&self, _c: Coord) -> bool {
        todo!()
    }

    pub fn is_empty(&self, _c: Coord) -> bool {
        todo!()
    }

    pub fn hash(&self) -> u64 {
        self.hash
    }

    pub fn ply(&self) -> u32 {
        self.ply
    }

    pub fn winner(&self) -> Option<Player> {
        todo!()
    }
}
