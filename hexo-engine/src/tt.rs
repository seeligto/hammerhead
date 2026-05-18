use crate::coords::Coord;

#[derive(Copy, Clone, Debug)]
pub enum TTFlag {
    Exact,
    LowerBound,
    UpperBound,
}

#[derive(Copy, Clone, Debug)]
pub struct TTEntry {
    /// Full 128-bit Zobrist hash. Bucket index is `(hash as u64) & MASK`;
    /// the full value is stored so probes can verify against collisions.
    pub hash: u128,
    pub depth: i8,
    pub score: i32,
    pub flag: TTFlag,
    pub best_move: Coord,
}

pub struct TranspositionTable {
    _entries: Vec<Option<TTEntry>>,
}

impl TranspositionTable {
    pub fn new(_size_mb: usize) -> Self {
        Self { _entries: Vec::new() }
    }

    pub fn probe(&self, _hash: u128) -> Option<TTEntry> {
        todo!()
    }

    pub fn store(&mut self, _entry: TTEntry) {
        todo!()
    }
}
