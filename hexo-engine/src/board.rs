//! `HeXO` board state.
//!
//! Owns piece map, candidate (legal-empty) set, history, 128-bit Zobrist hash,
//! and per-coord proximity counts. `place`/`undo` maintain candidates and hash
//! incrementally — no full scan.

use crate::axis_bitmap::AxisBitmaps;
use crate::config::{MAX_PIECE_DISTANCE, MOVE_GEN_INNER_RADIUS};
use crate::coords::{Coord, ORIGIN, for_each_in_range};
use crate::threats::{self, ThreatSet};
use crate::zobrist::ZobristTable;
use fxhash::{FxBuildHasher, FxHashMap, FxHashSet};
use std::cell::{Cell, Ref, RefCell};
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
    /// Per-cell count of pieces within `MAX_PIECE_DISTANCE`. Drives legality.
    proximity_count: FxHashMap<Coord, u32>,
    /// Per-cell count of pieces within `MOVE_GEN_INNER_RADIUS`. Drives the
    /// default-radius move generator without scanning every legal cell.
    inner_proximity_count: FxHashMap<Coord, u32>,
    candidate_cells: FxHashSet<Coord>,
    inner_candidate_cells: FxHashSet<Coord>,
    history: Vec<Coord>,
    hash: u128,
    ply: u32,
    zobrist: ZobristTable,
    axes: AxisBitmaps,
    winner: Option<Player>,
    /// Per-player lazily-computed threat caches. `None` means "stale; recompute
    /// on next read". `RefCell` so the public accessor on `&self` can fill
    /// the cache.
    threats_x: RefCell<Option<ThreatSet>>,
    threats_o: RefCell<Option<ThreatSet>>,
    /// Centre of the most recent change that invalidated the threat caches.
    /// Reserved for the Phase 8 incremental scanner.
    threats_dirty_center: Cell<Option<Coord>>,
    /// Lazily-filled static-eval result. `None` after every mutation,
    /// reassigned on the next call to [`Board::cached_eval`].
    eval_cache: Cell<Option<i32>>,
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
            inner_proximity_count: FxHashMap::with_capacity_and_hasher(
                INITIAL_MAP_CAPACITY,
                FxBuildHasher::default(),
            ),
            candidate_cells,
            inner_candidate_cells: FxHashSet::with_capacity_and_hasher(
                INITIAL_MAP_CAPACITY,
                FxBuildHasher::default(),
            ),
            history: Vec::with_capacity(INITIAL_MAP_CAPACITY),
            hash: 0,
            ply: 0,
            zobrist: ZobristTable::new(),
            axes: AxisBitmaps::new(),
            winner: None,
            threats_x: RefCell::new(None),
            threats_o: RefCell::new(None),
            threats_dirty_center: Cell::new(None),
            eval_cache: Cell::new(None),
        }
    }

    /// Reset to fresh state. Keeps the Zobrist table allocated.
    pub fn reset(&mut self) {
        self.pieces.clear();
        self.proximity_count.clear();
        self.inner_proximity_count.clear();
        self.candidate_cells.clear();
        self.candidate_cells.insert(ORIGIN);
        self.inner_candidate_cells.clear();
        self.history.clear();
        self.hash = 0;
        self.ply = 0;
        self.axes = AxisBitmaps::new();
        self.winner = None;
        self.threats_x.borrow_mut().take();
        self.threats_o.borrow_mut().take();
        self.threats_dirty_center.set(None);
        self.eval_cache.set(None);
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
        self.inner_candidate_cells.remove(&c);
        self.pieces.insert(c, player);

        add_proximity(
            &mut self.proximity_count,
            &mut self.candidate_cells,
            c,
            MAX_PIECE_DISTANCE,
            &self.pieces,
        );
        add_proximity(
            &mut self.inner_proximity_count,
            &mut self.inner_candidate_cells,
            c,
            MOVE_GEN_INNER_RADIUS,
            &self.pieces,
        );

        self.hash ^= self.zobrist.key(c, player);
        self.history.push(c);
        self.ply += 1;
        self.axes.set(c, player);
        self.invalidate_threats(c);
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
        self.invalidate_threats(c);
        if self.winner == Some(player) {
            self.winner = None;
        }

        self.hash ^= self.zobrist.key(c, player);
        self.ply -= 1;

        remove_proximity(
            &mut self.proximity_count,
            &mut self.candidate_cells,
            c,
            MAX_PIECE_DISTANCE,
        );
        remove_proximity(
            &mut self.inner_proximity_count,
            &mut self.inner_candidate_cells,
            c,
            MOVE_GEN_INNER_RADIUS,
        );

        if self.ply == 0 {
            // ply 0 rule: ORIGIN is the unique legal cell. `remove_proximity`
            // dropped it when its outer count fell to 0, so reinstate it here.
            // The inner set stays empty — move-gen short-circuits on
            // `ply == 0` before consulting it.
            self.candidate_cells.clear();
            self.candidate_cells.insert(ORIGIN);
            self.inner_candidate_cells.clear();
        } else {
            // c is empty again; re-add if still within range of some piece.
            if self.proximity_count.get(&c).copied().unwrap_or(0) > 0 {
                self.candidate_cells.insert(c);
            }
            if self.inner_proximity_count.get(&c).copied().unwrap_or(0) > 0 {
                self.inner_candidate_cells.insert(c);
            }
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

    /// Currently-legal empty cells. On an empty board this is `{ORIGIN}`;
    /// otherwise it is every empty cell within `MAX_PIECE_DISTANCE` of some
    /// piece. Maintained incrementally by `place` / `undo`.
    pub fn candidates(&self) -> impl Iterator<Item = Coord> + '_ {
        self.candidate_cells.iter().copied()
    }

    /// Empty cells within `MOVE_GEN_INNER_RADIUS` of some piece. Backs the
    /// default-radius move generator. Empty on an empty board (the move
    /// generator handles the empty-board case explicitly).
    pub fn inner_candidates(&self) -> impl Iterator<Item = Coord> + '_ {
        self.inner_candidate_cells.iter().copied()
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

    /// Threat snapshot for `player`. Cached on the board and refreshed on
    /// first read after any `place` / `undo`.
    ///
    /// Returns a `Ref` (not a `&ThreatSet`) because the cache lives behind
    /// a `RefCell`. Callers must hold the `Ref` for the duration of their
    /// access window.
    ///
    /// # Panics
    ///
    /// Panics if the cache is concurrently borrowed mutably — should never
    /// happen in single-threaded search code.
    #[must_use]
    pub fn threats(&self, player: Player) -> Ref<'_, ThreatSet> {
        let slot = match player {
            Player::X => &self.threats_x,
            Player::O => &self.threats_o,
        };
        if slot.borrow().is_none() {
            let center = self.threats_dirty_center.get();
            let fresh = threats::compute(self, player, center, None);
            *slot.borrow_mut() = Some(fresh);
        }
        Ref::map(slot.borrow(), |o| {
            o.as_ref().expect("filled above by lazy load")
        })
    }

    /// Drop the cached threat sets for both players and record `center` as
    /// the dirty origin. Called by `place` / `undo` after every mutation.
    fn invalidate_threats(&mut self, center: Coord) {
        self.threats_x.borrow_mut().take();
        self.threats_o.borrow_mut().take();
        self.threats_dirty_center.set(Some(center));
        self.eval_cache.set(None);
    }

    /// Static eval, cached on the board. Recomputes lazily after every
    /// `place` / `undo`. Use this from search leaves rather than calling
    /// [`crate::eval::eval`] directly.
    #[must_use]
    pub fn cached_eval(&self) -> i32 {
        if let Some(v) = self.eval_cache.get() {
            return v;
        }
        let v = crate::eval::eval(self);
        self.eval_cache.set(Some(v));
        v
    }

    /// Test-only: place a stone for an arbitrary player, bypassing the
    /// HeXO parity / turn rules. Updates every internal cache exactly as
    /// `place` would, including the threat dirty marker.
    ///
    /// Skips the empty-board and legal-range checks: callers are
    /// responsible for legality. **Do not call from production code.**
    #[doc(hidden)]
    pub fn place_for_test(&mut self, c: Coord, player: Player) {
        self.candidate_cells.remove(&c);
        self.inner_candidate_cells.remove(&c);
        self.pieces.insert(c, player);

        add_proximity(
            &mut self.proximity_count,
            &mut self.candidate_cells,
            c,
            MAX_PIECE_DISTANCE,
            &self.pieces,
        );
        add_proximity(
            &mut self.inner_proximity_count,
            &mut self.inner_candidate_cells,
            c,
            MOVE_GEN_INNER_RADIUS,
            &self.pieces,
        );

        self.hash ^= self.zobrist.key(c, player);
        self.history.push(c);
        self.ply += 1;
        self.axes.set(c, player);
        self.invalidate_threats(c);
        if crate::win::is_winning_move(self, c, player) {
            self.winner = Some(player);
        }
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

/// Increment proximity counts around `center` and insert any cell whose count
/// rose from 0 into `candidates` (if it's not already occupied).
///
/// Used to maintain both the outer (`r8`, legality) and inner
/// (`MOVE_GEN_INNER_RADIUS`, move-gen) refcounts via the exact same algorithm.
#[inline]
fn add_proximity(
    counts: &mut FxHashMap<Coord, u32>,
    candidates: &mut FxHashSet<Coord>,
    center: Coord,
    radius: i16,
    pieces: &FxHashMap<Coord, Player>,
) {
    for_each_in_range(center, radius, |d| {
        let count = counts.entry(d).or_insert(0);
        let was_zero = *count == 0;
        *count += 1;
        if d != center && was_zero && !pieces.contains_key(&d) {
            candidates.insert(d);
        }
    });
}

/// Decrement proximity counts around `center`. When a count reaches 0 the
/// entry is removed from `counts` and (if present) from `candidates`.
#[inline]
fn remove_proximity(
    counts: &mut FxHashMap<Coord, u32>,
    candidates: &mut FxHashSet<Coord>,
    center: Coord,
    radius: i16,
) {
    for_each_in_range(center, radius, |d| {
        let entry = counts
            .get_mut(&d)
            .expect("invariant: proximity_count entry exists for neighbour");
        *entry -= 1;
        if *entry == 0 {
            counts.remove(&d);
            candidates.remove(&d);
        }
    });
}
