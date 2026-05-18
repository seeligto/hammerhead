//! `HeXO` board state.
//!
//! Owns piece map, candidate (legal-empty) set, history, 128-bit Zobrist hash,
//! and per-coord proximity counts. `place`/`undo` maintain candidates and hash
//! incrementally — no full scan.

use crate::axis_bitmap::AxisBitmaps;
use crate::config::MAX_PIECE_DISTANCE;
use crate::coords::{Coord, ORIGIN, for_each_in_range};
use crate::zobrist::ZobristTable;
use fxhash::{FxBuildHasher, FxHashMap, FxHashSet};
use thiserror::Error;

/// Players. Discriminant doubles as Zobrist player index.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
#[repr(u8)]
pub enum Player {
    X = 0,
    O = 1,
}

impl Player {
    /// Other player.
    #[inline]
    #[must_use]
    pub const fn opponent(self) -> Player {
        match self {
            Player::X => Player::O,
            Player::O => Player::X,
        }
    }
}

/// Reason a `place`/`undo` failed.
#[derive(Error, Debug, PartialEq, Eq)]
pub enum BoardError {
    #[error("cell ({0}, {1}) is already occupied")]
    AlreadyOccupied(i16, i16),
    #[error("first move must be at origin (0, 0), got ({0}, {1})")]
    MustStartAtOrigin(i16, i16),
    #[error("cell ({0}, {1}) is out of legal range (>{2} from any piece)")]
    OutOfRange(i16, i16, i16),
    #[error("no moves to undo")]
    NoHistory,
}

/// Initial capacity for piece-keyed maps. Avoids rehashing during a typical
/// game (`HeXO` games rarely exceed ~256 stones).
const INITIAL_MAP_CAPACITY: usize = 256;

/// `HeXO` board state.
pub struct Board {
    pieces: FxHashMap<Coord, Player>,
    proximity_count: FxHashMap<Coord, u32>,
    candidate_cells: FxHashSet<Coord>,
    history: Vec<Coord>,
    hash: u128,
    ply: u32,
    zobrist: ZobristTable,
    axes: AxisBitmaps,
    winner: Option<Player>,
}

impl Default for Board {
    fn default() -> Self {
        Self::new()
    }
}

impl Board {
    /// Empty board. Single candidate `ORIGIN`. X to move, ply 0, hash 0.
    #[must_use]
    pub fn new() -> Self {
        let mut candidate_cells: FxHashSet<Coord> =
            FxHashSet::with_capacity_and_hasher(INITIAL_MAP_CAPACITY, FxBuildHasher::default());
        candidate_cells.insert(ORIGIN);
        Self {
            pieces: FxHashMap::with_capacity_and_hasher(
                INITIAL_MAP_CAPACITY,
                FxBuildHasher::default(),
            ),
            proximity_count: FxHashMap::with_capacity_and_hasher(
                INITIAL_MAP_CAPACITY,
                FxBuildHasher::default(),
            ),
            candidate_cells,
            history: Vec::with_capacity(INITIAL_MAP_CAPACITY),
            hash: 0,
            ply: 0,
            zobrist: ZobristTable::new(),
            axes: AxisBitmaps::new(),
            winner: None,
        }
    }

    /// Reset to fresh state. Keeps the Zobrist table allocated.
    pub fn reset(&mut self) {
        self.pieces.clear();
        self.proximity_count.clear();
        self.candidate_cells.clear();
        self.candidate_cells.insert(ORIGIN);
        self.history.clear();
        self.hash = 0;
        self.ply = 0;
        self.axes = AxisBitmaps::new();
        self.winner = None;
    }

    /// Place the next stone at `c`. Updates hash, candidates, history.
    ///
    /// # Errors
    ///
    /// Returns `BoardError::MustStartAtOrigin` on a non-origin first move,
    /// `BoardError::AlreadyOccupied` if `c` is taken, or
    /// `BoardError::OutOfRange` if `c` is farther than `MAX_PIECE_DISTANCE`
    /// from every existing piece.
    pub fn place(&mut self, c: Coord) -> Result<(), BoardError> {
        if self.ply == 0 {
            if c != ORIGIN {
                return Err(BoardError::MustStartAtOrigin(c.q, c.r));
            }
        } else {
            if self.pieces.contains_key(&c) {
                return Err(BoardError::AlreadyOccupied(c.q, c.r));
            }
            if !self.is_legal_internal(c) {
                return Err(BoardError::OutOfRange(c.q, c.r, MAX_PIECE_DISTANCE));
            }
        }

        let player = self.to_move();

        self.candidate_cells.remove(&c);
        self.pieces.insert(c, player);

        let pieces = &self.pieces;
        let proximity_count = &mut self.proximity_count;
        let candidate_cells = &mut self.candidate_cells;
        for_each_in_range(c, MAX_PIECE_DISTANCE, |d| {
            let count = proximity_count.entry(d).or_insert(0);
            let was_zero = *count == 0;
            *count += 1;
            if d != c && was_zero && !pieces.contains_key(&d) {
                candidate_cells.insert(d);
            }
        });

        self.hash ^= self.zobrist.key(c, player);
        self.history.push(c);
        self.ply += 1;
        self.axes.set(c, player);
        if crate::win::is_winning_move(self, c, player) {
            self.winner = Some(player);
        }
        Ok(())
    }

    /// Undo the most recent placement.
    ///
    /// # Errors
    ///
    /// Returns `BoardError::NoHistory` when there is nothing to undo.
    ///
    /// # Panics
    ///
    /// Panics if internal invariants are violated (history entry missing
    /// from the pieces map or proximity map). These should be unreachable.
    pub fn undo(&mut self) -> Result<(), BoardError> {
        let c = self.history.pop().ok_or(BoardError::NoHistory)?;
        let player = self
            .pieces
            .remove(&c)
            .expect("invariant: history piece in pieces map");

        self.axes.clear(c, player);
        if self.winner == Some(player) {
            self.winner = None;
        }

        self.hash ^= self.zobrist.key(c, player);
        self.ply -= 1;

        let proximity_count = &mut self.proximity_count;
        let candidate_cells = &mut self.candidate_cells;
        for_each_in_range(c, MAX_PIECE_DISTANCE, |d| {
            let entry = proximity_count
                .get_mut(&d)
                .expect("invariant: proximity_count entry exists for r8 neighbour");
            *entry -= 1;
            if *entry == 0 {
                proximity_count.remove(&d);
                candidate_cells.remove(&d);
            }
        });

        if self.ply == 0 {
            // Board empty: only ORIGIN is legal.
            self.candidate_cells.clear();
            self.candidate_cells.insert(ORIGIN);
        } else if self.proximity_count.get(&c).copied().unwrap_or(0) > 0 {
            // c is empty again and still within r8 of some remaining piece.
            self.candidate_cells.insert(c);
        }

        Ok(())
    }

    /// Total stones placed so far.
    #[inline]
    #[must_use]
    pub fn ply(&self) -> u32 {
        self.ply
    }

    /// 128-bit Zobrist hash of the position.
    #[inline]
    #[must_use]
    pub fn hash(&self) -> u128 {
        self.hash
    }

    /// Player who places the next stone.
    #[inline]
    #[must_use]
    pub fn to_move(&self) -> Player {
        player_at_ply(self.ply)
    }

    /// Number of stones on the board.
    #[inline]
    #[must_use]
    pub fn piece_count(&self) -> usize {
        self.pieces.len()
    }

    /// `true` iff `c` has no stone.
    #[inline]
    #[must_use]
    pub fn is_empty_cell(&self, c: Coord) -> bool {
        !self.pieces.contains_key(&c)
    }

    /// Player on `c`, or `None` if empty.
    #[inline]
    #[must_use]
    pub fn piece_at(&self, c: Coord) -> Option<Player> {
        self.pieces.get(&c).copied()
    }

    /// `true` iff placing at `c` would succeed.
    #[inline]
    #[must_use]
    pub fn is_legal(&self, c: Coord) -> bool {
        if !self.is_empty_cell(c) {
            return false;
        }
        if self.ply == 0 {
            return c == ORIGIN;
        }
        self.is_legal_internal(c)
    }

    /// Empty cells within `r8` of some piece. May include cells that fail the
    /// `ply == 0` rule when called on an empty board — see `is_legal`.
    pub fn candidates(&self) -> impl Iterator<Item = Coord> + '_ {
        self.candidate_cells.iter().copied()
    }

    /// All placed pieces.
    pub fn pieces(&self) -> impl Iterator<Item = (Coord, Player)> + '_ {
        self.pieces.iter().map(|(&c, &p)| (c, p))
    }

    /// Move history in placement order.
    #[inline]
    #[must_use]
    pub fn history(&self) -> &[Coord] {
        &self.history
    }

    /// Per-axis per-player line bitmaps. Used by win detection and eval.
    #[inline]
    #[must_use]
    pub fn axes(&self) -> &AxisBitmaps {
        &self.axes
    }

    /// Player who just won, if any. `Some(p)` iff the most recent
    /// non-undone `place` produced a 6-in-row for `p`.
    #[inline]
    #[must_use]
    pub fn winner(&self) -> Option<Player> {
        self.winner
    }

    /// Internal radius check. Assumes ply >= 1.
    #[inline]
    fn is_legal_internal(&self, c: Coord) -> bool {
        self.proximity_count.get(&c).copied().unwrap_or(0) > 0
    }
}

/// Map a ply index to the player who plays that ply (see `SPEC_ENGINE.md`).
#[inline]
#[must_use]
pub fn player_at_ply(p: u32) -> Player {
    if p == 0 {
        Player::X
    } else if ((p - 1) / 2) % 2 == 0 {
        Player::O
    } else {
        Player::X
    }
}
