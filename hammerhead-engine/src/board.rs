//! `HeXO` board state.
//!
//! Owns piece map, candidate (legal-empty) set, history, 128-bit Zobrist hash,
//! and per-coord proximity counts. `place`/`undo` maintain candidates and hash
//! incrementally — no full scan.

use crate::axis_bitmap::AxisBitmaps;
use crate::config::{MAX_PIECE_DISTANCE, MOVE_GEN_INNER_RADIUS};
use crate::coords::{Coord, ORIGIN};
use crate::eval_overrides::{EvalOverrides, WINDOW_SCORE_8_LEN};
use crate::line_contrib::LineContrib;
use crate::proximity::{ProximityCounts, SparseCellSet, add_proximity, remove_proximity};
use crate::threats::{self, ThreatScratch, ThreatSet};
use crate::zobrist::{Z_HALFMOVE, Z_TURN_X, ZobristTable};
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

/// Initial capacity for the history vectors. Avoids reallocation during a
/// typical game (`HeXO` games rarely exceed ~256 stones).
const INITIAL_MAP_CAPACITY: usize = 256;

/// `HeXO` board state.
pub struct Board {
    /// Per-cell proximity refcounts: outer (r=8, legality) + inner
    /// (r=2, move-gen), as flat `u8` arrays. Phase 16 replaced the
    /// coord-keyed `FxHashMap` pair. See `SPEC_ENGINE.md
    /// § Candidate maintenance`.
    proximity: ProximityCounts,
    /// Currently-legal empty cells (outer r=8). Phase 16 flat structure
    /// replacing the `FxHashSet`.
    candidates: SparseCellSet,
    /// Move-gen cells (inner r=2). Phase 16 flat structure.
    inner_candidates: SparseCellSet,
    history: Vec<Coord>,
    /// Player parallel to `history`: `history_players[i]` is the player
    /// whose stone was placed at `history[i]`. Phase 13 replaced the
    /// per-cell `FxHashMap<Coord, Player>` with this parallel vector so
    /// `undo` can recover the placed player without a hash probe and
    /// without depending on `player_at_ply` (which `place_for_test` may
    /// override).
    history_players: Vec<Player>,
    hash: u128,
    ply: u32,
    /// 0 = side-to-move is about to place stone 1 of their turn;
    /// 1 = about to place stone 2. See `SPEC_ENGINE.md` "Zobrist hashing".
    halfmove: u8,
    /// Explicit cache of `to_move()`. Maintained in lockstep with `ply`
    /// and `halfmove`; debug builds assert agreement with the parity formula.
    side_to_move: Player,
    zobrist: ZobristTable,
    axes: AxisBitmaps,
    winner: Option<Player>,
    /// Per-player threat caches. Always populated after construction
    /// — invariant maintained by [`Board::reset`] / [`Board::new`].
    /// The `threats_dirty` flag tracks whether the cached value is
    /// current; when `dirty == false`, the cached `ThreatSet` is
    /// authoritative. `RefCell` so the public accessor on `&self` can
    /// fill the cache on a dirty read.
    ///
    /// Phase 15 STEP 3: dropped the `Option` wrapper. The Phase-14
    /// flamegraph's `pvs_node;threats;is_none;is_some<ThreatSet>` frame
    /// disappeared once the `Option` projection inside `Ref::map` was
    /// eliminated. The invariant is now load-bearing for safety: every
    /// site that mutated the cache (`new`, `reset`, `reconcile_threats`)
    /// writes a `ThreatSet`, not an `Option`.
    threats_x: RefCell<ThreatSet>,
    threats_o: RefCell<ThreatSet>,
    /// Reusable scratch buffers for the threat recompute hot path. Reset
    /// at the top of every `compute_with_scratch` call so capacity is
    /// retained across nodes.
    threat_scratch: RefCell<ThreatScratch>,
    /// Dirty flag. `true` ⟹ cached threats are stale and must be
    /// recomputed on next read. `Cell<bool>` (not `RefCell<bool>`) so the
    /// hot-path cache-clean check is a single byte load with no borrow
    /// tracking.
    threats_dirty: Cell<bool>,
    /// Lazily-filled static-eval result. `None` after every mutation,
    /// reassigned on the next call to [`Board::cached_eval`].
    eval_cache: Cell<Option<i32>>,
    /// Phase-27 per-`(axis, line_id)` Layer-1 contribution cache.
    /// Scaffold only in C-01 — no consumers, no invalidation hooks yet.
    /// `RefCell` mirrors the `threats_x` / `threats_o` pattern: hot-path
    /// reads on `&Board` will populate the cache lazily on miss.
    line_contrib: RefCell<LineContrib>,
    /// Phase 28B-1 runtime eval-weight overrides. `Default` mirrors the
    /// codegen'd `crate::config::*` constants, so an unset override is
    /// byte-identical to the non-tuning build (gate: reference node
    /// counts unchanged).
    eval_overrides: Cell<EvalOverrides>,
    /// Runtime `WINDOW_SCORE_8` table mirror. `None` ⟹ Layer 1 reads the
    /// build.rs-codegen'd `crate::config::WINDOW_SCORE_8`. `Some(box)` ⟹
    /// the override changed Layer-1 inputs and `set_eval_overrides`
    /// materialised a fresh 6561-entry table. Cost: one ~26 KB heap
    /// allocation per `set_eval_overrides` call where Layer-1 inputs
    /// changed — amortised across a whole match, never per node.
    window_score_table: RefCell<Option<Box<[i32; WINDOW_SCORE_8_LEN]>>>,
}

impl Default for Board {
    fn default() -> Self {
        Self::new()
    }
}

impl Board {
    /// Empty board. Single candidate `ORIGIN`. X to move, ply 0,
    /// halfmove 0; initial hash is the X-turn parity overlay
    /// ([`Z_TURN_X`]).
    #[must_use]
    pub fn new() -> Self {
        let mut candidates = SparseCellSet::new();
        candidates.insert(ORIGIN);
        Self {
            proximity: ProximityCounts::new(),
            candidates,
            inner_candidates: SparseCellSet::new(),
            history: Vec::with_capacity(INITIAL_MAP_CAPACITY),
            history_players: Vec::with_capacity(INITIAL_MAP_CAPACITY),
            hash: Z_TURN_X,
            ply: 0,
            halfmove: 0,
            side_to_move: Player::X,
            zobrist: ZobristTable::new(),
            axes: AxisBitmaps::new(),
            winner: None,
            threats_x: RefCell::new(ThreatSet::default()),
            threats_o: RefCell::new(ThreatSet::default()),
            threat_scratch: RefCell::new(ThreatScratch::default()),
            threats_dirty: Cell::new(false),
            eval_cache: Cell::new(None),
            line_contrib: RefCell::new(LineContrib::new()),
            eval_overrides: Cell::new(EvalOverrides::default()),
            window_score_table: RefCell::new(None),
        }
    }

    /// Reset to fresh state. Keeps the Zobrist table allocated.
    pub fn reset(&mut self) {
        self.proximity.clear();
        self.candidates.clear();
        self.candidates.insert(ORIGIN);
        self.inner_candidates.clear();
        self.history.clear();
        self.history_players.clear();
        self.hash = Z_TURN_X;
        self.ply = 0;
        self.halfmove = 0;
        self.side_to_move = Player::X;
        self.axes = AxisBitmaps::new();
        self.winner = None;
        *self.threats_x.borrow_mut() = ThreatSet::default();
        *self.threats_o.borrow_mut() = ThreatSet::default();
        self.threats_dirty.set(false);
        self.threat_scratch.borrow_mut().clear_all();
        self.eval_cache.set(None);
        self.line_contrib.borrow_mut().reset();
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
            if self.axes.is_occupied(c) {
                return Err(BoardError::AlreadyOccupied(c.q, c.r));
            }
            if !self.is_legal_internal(c) {
                return Err(BoardError::OutOfRange(c.q, c.r, MAX_PIECE_DISTANCE));
            }
        }

        let player = self.side_to_move;

        self.candidates.remove(c);
        self.inner_candidates.remove(c);
        // `axes.set` must happen before `add_proximity` so the occupancy
        // probe inside the proximity loop sees the new stone (replaces the
        // old `pieces.insert(c, player)` that preceded `add_proximity`).
        self.apply_set(c, player);

        add_proximity(
            &mut self.proximity.outer,
            &mut self.candidates,
            c,
            MAX_PIECE_DISTANCE,
            &self.axes,
        );
        add_proximity(
            &mut self.proximity.inner,
            &mut self.inner_candidates,
            c,
            MOVE_GEN_INNER_RADIUS,
            &self.axes,
        );

        self.hash ^= self.zobrist.key(c, player);
        self.history.push(c);
        self.history_players.push(player);
        self.ply += 1;
        self.advance_parity();
        self.mark_threats_dirty();
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
    /// Panics if internal invariants are violated (`history_players`
    /// desync or missing proximity-map entry). These should be unreachable.
    pub fn undo(&mut self) -> Result<(), BoardError> {
        let c = self.history.pop().ok_or(BoardError::NoHistory)?;
        let player = self
            .history_players
            .pop()
            .expect("invariant: history_players parallel to history");

        self.apply_clear(c, player);
        self.mark_threats_dirty();
        if self.winner == Some(player) {
            self.winner = None;
        }

        self.hash ^= self.zobrist.key(c, player);
        self.ply -= 1;
        self.retreat_parity();

        remove_proximity(
            &mut self.proximity.outer,
            &mut self.candidates,
            c,
            MAX_PIECE_DISTANCE,
        );
        remove_proximity(
            &mut self.proximity.inner,
            &mut self.inner_candidates,
            c,
            MOVE_GEN_INNER_RADIUS,
        );

        if self.ply == 0 {
            // ply 0 rule: ORIGIN is the unique legal cell. `remove_proximity`
            // dropped it when its outer count fell to 0, so reinstate it here.
            // The inner set stays empty — move-gen short-circuits on
            // `ply == 0` before consulting it.
            self.candidates.clear();
            self.candidates.insert(ORIGIN);
            self.inner_candidates.clear();
        } else {
            // c is empty again; re-add if still within range of some piece.
            if self.proximity.outer_at(c) > 0 {
                self.candidates.insert(c);
            }
            if self.proximity.inner_at(c) > 0 {
                self.inner_candidates.insert(c);
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
        debug_assert_eq!(
            self.side_to_move,
            player_at_ply(self.ply),
            "side_to_move desync at ply {}",
            self.ply,
        );
        self.side_to_move
    }

    /// Halfmove flag: 0 = side-to-move is about to play stone 1 of their
    /// turn; 1 = about to play stone 2. See [`crate::zobrist`].
    #[inline]
    #[must_use]
    pub fn halfmove(&self) -> u8 {
        self.halfmove
    }

    /// Number of stones on the board.
    #[inline]
    #[must_use]
    pub fn piece_count(&self) -> usize {
        self.history.len()
    }

    /// `true` iff `c` has no stone.
    #[inline]
    #[must_use]
    pub fn is_empty_cell(&self, c: Coord) -> bool {
        !self.axes.is_occupied(c)
    }

    /// Player on `c`, or `None` if empty.
    #[inline]
    #[must_use]
    pub fn piece_at(&self, c: Coord) -> Option<Player> {
        self.axes.player_at(c)
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
        self.candidates.iter()
    }

    /// Empty cells within `MOVE_GEN_INNER_RADIUS` of some piece. Backs the
    /// default-radius move generator. Empty on an empty board (the move
    /// generator handles the empty-board case explicitly).
    pub fn inner_candidates(&self) -> impl Iterator<Item = Coord> + '_ {
        self.inner_candidates.iter()
    }

    /// All placed pieces, in insertion order. Phase 13 replaced the prior
    /// `FxHashMap<Coord, Player>` random-order iteration with an
    /// insertion-ordered walk over `history` + `history_players`; the
    /// pre-Phase-13 callsite scan
    /// (`subagents/scans/phase13-piece-at-callsites.md`) verified every
    /// caller is order-insensitive.
    pub fn pieces(&self) -> impl Iterator<Item = (Coord, Player)> + '_ {
        self.history
            .iter()
            .copied()
            .zip(self.history_players.iter().copied())
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

    /// Phase-27 per-`(axis, line_id)` Layer-1 contribution cache. Consumed
    /// by `eval::layer1_window_scan_8cell` to skip the per-line ternary
    /// window scan on a cache hit. Behind a `RefCell` so the eval hot-path
    /// can populate misses lazily through `&Board`.
    #[inline]
    pub(crate) fn line_contrib(&self) -> &RefCell<LineContrib> {
        &self.line_contrib
    }

    /// Snapshot of the current runtime eval-weight overrides. `Copy`
    /// — 56 bytes, fits in one cache line; no borrow tracking.
    /// Defaults to `EvalOverrides::default()` (codegen'd constants), so
    /// callers that never call [`Self::set_eval_overrides`] observe the
    /// build-time defaults.
    #[inline]
    #[must_use]
    pub fn eval_overrides(&self) -> EvalOverrides {
        self.eval_overrides.get()
    }

    /// Runtime Layer-1 `WINDOW_SCORE_8` table accessor. Returns the
    /// override-derived table if [`Self::set_eval_overrides`] has changed
    /// any Layer-1 input; otherwise `None` and the caller falls back to
    /// the codegen'd `crate::config::WINDOW_SCORE_8`. `RefCell` because
    /// the table lives in a heap allocation owned by the board.
    #[inline]
    pub(crate) fn window_score_table(
        &self,
    ) -> std::cell::Ref<'_, Option<Box<[i32; WINDOW_SCORE_8_LEN]>>> {
        self.window_score_table.borrow()
    }

    /// Replace the runtime eval-weight overrides. Invalidates the
    /// Phase-27 `LineContribution` cache, the lazy static-eval cache,
    /// and the cached threats so Layer 2/3 re-read the new S0/fork
    /// weights. If Layer-1 inputs differ from the previous override,
    /// rebuilds the runtime `WINDOW_SCORE_8` table (microseconds —
    /// amortised across the match).
    ///
    /// Persists across [`Self::reset`] (Phase 18 precedent).
    pub fn set_eval_overrides(&mut self, new_overrides: EvalOverrides) {
        let old = self.eval_overrides.get();
        self.eval_overrides.set(new_overrides);
        let defaults = EvalOverrides::default();
        // Layer-1 inputs unchanged ⟹ no table rebuild needed. When the
        // new overrides happen to match defaults bit-for-bit, clear any
        // stale runtime table so Layer-1 reads from the codegen'd
        // `WINDOW_SCORE_8` (preserving the byte-identical-default gate).
        if new_overrides == defaults {
            *self.window_score_table.borrow_mut() = None;
        } else if !new_overrides.layer1_inputs_eq(&old)
            || self.window_score_table.borrow().is_none()
        {
            *self.window_score_table.borrow_mut() =
                Some(new_overrides.build_window_score_8());
        }
        // Any S0 / window / extension / fork change perturbs cached
        // contributions and downstream eval. Reuse the existing Phase-27
        // cache-invalidation pattern.
        self.line_contrib.borrow_mut().reset();
        self.mark_threats_dirty();
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
        self.proximity.outer_at(c) > 0
    }

    /// Advance `(side_to_move, halfmove)` one stone forward and XOR the
    /// resulting parity overlay into `self.hash`.
    ///
    /// Must be called after `self.ply` has been incremented; the post-ply
    /// value is used to decide the X-singleton edge case.
    #[inline]
    fn advance_parity(&mut self) {
        let old = parity_overlay(self.side_to_move, self.halfmove);
        let (new_side, new_half) = next_parity(self.side_to_move, self.halfmove, self.ply);
        self.side_to_move = new_side;
        self.halfmove = new_half;
        let new = parity_overlay(new_side, new_half);
        self.hash ^= old ^ new;
    }

    /// Reverse of [`Self::advance_parity`]: back-derive `(side, halfmove)` from
    /// the CURRENT (post-advance) parity, not from `player_at_ply`. Using
    /// the natural-parity formula would clobber any state set by
    /// [`Board::force_parity_for_test`] across a `place`/`undo` cycle.
    ///
    /// Must be called after `self.ply` has been decremented; the
    /// pre-decrement value (`self.ply + 1`) is the `post_ply` that
    /// `advance_parity` saw.
    #[inline]
    fn retreat_parity(&mut self) {
        let old = parity_overlay(self.side_to_move, self.halfmove);
        let (new_side, new_half) = prev_parity(self.side_to_move, self.halfmove, self.ply + 1);
        self.side_to_move = new_side;
        self.halfmove = new_half;
        let new = parity_overlay(new_side, new_half);
        self.hash ^= old ^ new;
    }

    /// Threat snapshot for `player`. Cached on the board and refreshed on
    /// first read after any `place` / `undo`.
    ///
    /// Phase 15 fast path: when [`Self::threats_dirty`] is `false`, the
    /// cached `ThreatSet` is authoritative and is returned with a single
    /// `RefCell::borrow` + `Ref::map`. No `Option::is_none` check, no
    /// compute call.
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
        if self.threats_dirty.get() {
            self.reconcile_threats();
        }
        match player {
            Player::X => self.threats_x.borrow(),
            Player::O => self.threats_o.borrow(),
        }
    }

    /// Recompute both player caches and clear the dirty flag. Called only
    /// from [`Self::threats`] when the dirty flag is set.
    #[cold]
    fn reconcile_threats(&self) {
        let mut scratch = self.threat_scratch.borrow_mut();

        // Player X.
        {
            let new_x = threats::compute_with_scratch(self, Player::X, &mut scratch);
            *self.threats_x.borrow_mut() = new_x;
        }

        // Player O — symmetric.
        {
            let new_o = threats::compute_with_scratch(self, Player::O, &mut scratch);
            *self.threats_o.borrow_mut() = new_o;
        }

        drop(scratch);
        self.clear_threats_dirty();
    }

    /// Mutate `self.axes` for a stone placement and invalidate the 3
    /// `LineContribution` cache lines through `c` in lockstep. The only
    /// permitted writer to `self.axes` from `place` / `place_for_test`;
    /// keeps the two structures consistent without per-site bookkeeping.
    #[inline]
    fn apply_set(&mut self, c: Coord, player: Player) {
        self.axes.set(c, player);
        self.line_contrib.borrow_mut().invalidate_coord(c);
    }

    /// Symmetric inverse of [`Self::apply_set`] for `undo`. Same
    /// `LineContribution`-invalidation contract.
    #[inline]
    fn apply_clear(&mut self, c: Coord, player: Player) {
        self.axes.clear(c, player);
        self.line_contrib.borrow_mut().invalidate_coord(c);
    }

    /// Mark the threat caches as stale so the next [`Self::threats`] read
    /// triggers a recompute.
    #[inline]
    fn mark_threats_dirty(&self) {
        self.threats_dirty.set(true);
        // The eval cache shadows the same board state; invalidate together.
        self.eval_cache.set(None);
    }

    /// Clear the dirty flag. Called only from [`Self::reconcile_threats`]
    /// after a successful recompute.
    #[inline]
    fn clear_threats_dirty(&self) {
        self.threats_dirty.set(false);
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
        self.candidates.remove(c);
        self.inner_candidates.remove(c);
        // Mirror `place`: axes.set before add_proximity so the occupancy
        // probe inside the proximity loop sees the new stone.
        self.apply_set(c, player);

        add_proximity(
            &mut self.proximity.outer,
            &mut self.candidates,
            c,
            MAX_PIECE_DISTANCE,
            &self.axes,
        );
        add_proximity(
            &mut self.proximity.inner,
            &mut self.inner_candidates,
            c,
            MOVE_GEN_INNER_RADIUS,
            &self.axes,
        );

        self.hash ^= self.zobrist.key(c, player);
        self.history.push(c);
        self.history_players.push(player);
        self.ply += 1;
        self.advance_parity();
        self.mark_threats_dirty();
        if crate::win::is_winning_move(self, c, player) {
            self.winner = Some(player);
        }
    }

    /// Test-only: read the threats-dirty flag. `false` ⟹ both player
    /// caches are current.
    #[doc(hidden)]
    #[must_use]
    pub fn threats_dirty_for_test(&self) -> bool {
        self.threats_dirty.get()
    }

    /// Test-only: overwrite `(side_to_move, halfmove)` and re-overlay the
    /// resulting parity bits into `self.hash`. Used by zobrist tests to
    /// construct hypothetical parity-distinct positions that are not
    /// reachable from a normal game start.
    ///
    /// **Do not call from production code** — leaves the parity desynced
    /// from `ply`, which trips the `debug_assert` inside `to_move()`.
    #[doc(hidden)]
    pub fn force_parity_for_test(&mut self, side: Player, halfmove: u8) {
        debug_assert!(halfmove <= 1, "halfmove must be 0 or 1");
        let old = parity_overlay(self.side_to_move, self.halfmove);
        let new = parity_overlay(side, halfmove);
        self.hash ^= old ^ new;
        self.side_to_move = side;
        self.halfmove = halfmove;
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

/// Halfmove flag derived from ply. `0` before any stone is placed, then
/// `(p + 1) % 2` for `p >= 1` (encodes the X-singleton exception at ply 1).
#[inline]
#[must_use]
pub fn halfmove_at_ply(p: u32) -> u8 {
    if p == 0 { 0 } else { ((p + 1) & 1) as u8 }
}

/// Parity overlay XOR'd into `Board::hash` given a `(side, halfmove)`
/// state. See `SPEC_ENGINE.md` "Zobrist hashing".
///
/// Branch-free: each predicate widens to a `u128` whose two's-complement
/// negation is either all-zeros or all-ones, then is `AND`-ed against
/// the corresponding parity constant.
#[inline]
fn parity_overlay(side: Player, halfmove: u8) -> u128 {
    let x_mask = u128::from(matches!(side, Player::X)).wrapping_neg();
    let h_mask = u128::from(halfmove & 1).wrapping_neg();
    (Z_TURN_X & x_mask) ^ (Z_HALFMOVE & h_mask)
}

/// Successor of `(side, halfmove)` after one stone is placed.
///
/// `post_ply` is the ply count **after** the stone, i.e. `self.ply`
/// once it has been incremented. The only case that depends on it is
/// the X-singleton rule: `(X, 0) → (O, 0)` instead of `(X, 1)` after
/// the opening stone, which is exactly the `post_ply == 1` transition.
#[inline]
#[must_use]
fn next_parity(side: Player, halfmove: u8, post_ply: u32) -> (Player, u8) {
    match (side, halfmove) {
        (Player::X, 0) if post_ply == 1 => (Player::O, 0),
        (Player::X, 0) => (Player::X, 1),
        (Player::X, _) => (Player::O, 0),
        (Player::O, 0) => (Player::O, 1),
        (Player::O, _) => (Player::X, 0),
    }
}

/// Inverse of [`next_parity`]. Given the parity *after* an advance and
/// the `post_ply` value the advance saw, return the parity *before* the
/// advance. Used by [`Board::retreat_parity`] so a `place`/`undo` pair
/// round-trips through arbitrary starting parities (in particular,
/// states set up by [`Board::force_parity_for_test`]).
#[inline]
#[must_use]
fn prev_parity(side: Player, halfmove: u8, post_ply: u32) -> (Player, u8) {
    match (side, halfmove) {
        (Player::O, 0) if post_ply == 1 => (Player::X, 0),
        (Player::X, 0) => (Player::O, 1),
        (Player::X, _) => (Player::X, 0),
        (Player::O, 0) => (Player::X, 1),
        (Player::O, _) => (Player::O, 0),
    }
}

