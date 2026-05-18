use crate::board::Player;
use crate::coords::Coord;

pub struct ZobristTable {
    _private: (),
}

impl Default for ZobristTable {
    fn default() -> Self {
        Self::new()
    }
}

impl ZobristTable {
    pub fn new() -> Self {
        Self { _private: () }
    }

    pub fn key(&self, _coord: Coord, _player: Player) -> u64 {
        todo!()
    }
}
