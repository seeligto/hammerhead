//! Engine handle — owning bundle of per-game search state.
//!
//! `Engine` is the public entry point for callers that want the full
//! game-loop API (place / undo / `best_move` / `find_pv` / reset / `clear_tt`).
//! The actual search algorithm lives in [`crate::search`].

#![allow(clippy::must_use_candidate)]
#![allow(clippy::cast_sign_loss)]

use crate::board::{Board, BoardError, Player};
use crate::config::DEFAULT_TT_SIZE_MB;
use crate::coords::Coord;
use crate::eval_overrides::EvalOverrides;
use crate::ordering::OrderingState;
use crate::search::{SearchConfig, SearchResult, SearchScratch, search_root};
use crate::tt::TranspositionTable;

// ─────────────────────────────────────────────────────────────────────────────
// Engine entry point (wrapped by `pybind.rs` in Phase 9)
// ─────────────────────────────────────────────────────────────────────────────

/// Owning bundle of the per-game search state. The `PyO3` layer holds
/// one of these behind a `#[pyclass]` shim.
pub struct Engine {
    /// Mutable board with proximity / threat / eval caches.
    pub board: Board,
    /// Two-bucket TT shared across consecutive `best_move` calls.
    pub tt: TranspositionTable,
    /// Killer + history state, persisted between calls.
    pub ordering: OrderingState,
    /// Per-ply scratch buffers for move generation, sort scratch, bucket
    /// values, and qsearch threat sub-lists. Capacity is retained across
    /// `best_move` calls so the search hot path is allocation-free after
    /// warmup.
    pub scratch: SearchScratch,
    /// Tunable search parameters (defaults sourced from `hexo.toml`).
    pub cfg: SearchConfig,
}

impl Engine {
    /// Construct an engine with a TT of approximately `tt_size_mb` MB.
    /// `0` falls back to `DEFAULT_TT_SIZE_MB`.
    #[must_use]
    pub fn new(tt_size_mb: usize) -> Self {
        let mb = if tt_size_mb == 0 {
            DEFAULT_TT_SIZE_MB
        } else {
            tt_size_mb
        };
        Self {
            board: Board::new(),
            tt: TranspositionTable::new(mb),
            ordering: OrderingState::new(),
            scratch: SearchScratch::new(),
            cfg: SearchConfig::default(),
        }
    }

    /// Place a stone at `c` for the side currently to move.
    ///
    /// # Errors
    ///
    /// Forwards [`BoardError`] from [`Board::place`].
    pub fn place(&mut self, c: Coord) -> Result<(), BoardError> {
        self.board.place(c)
    }

    /// Undo the most recent placement.
    ///
    /// # Errors
    ///
    /// Forwards [`BoardError`] from [`Board::undo`].
    pub fn undo(&mut self) -> Result<(), BoardError> {
        self.board.undo()
    }

    /// Search the current position and return the chosen move.
    ///
    /// `time_ms` overrides `cfg.time_ms` for this call; `depth` overrides
    /// `cfg.max_depth`. When both are `None`, defaults apply.
    ///
    /// `time_ms` is the **per-stone** budget: the engine consumes the
    /// whole value on this single `best_move` call and does not split it.
    /// Callers issue one `best_move` per stone (two per turn).
    pub fn best_move(&mut self, time_ms: Option<u64>, depth: Option<i8>) -> SearchResult {
        let mut local = self.cfg;
        if let Some(d) = depth {
            local.max_depth = d.max(1);
        }
        // A depth-only call is honoured as a fixed-depth search with no
        // time limit, matching the SPEC_BENCHMARKS reference contract.
        // The default `cfg.time_ms` only kicks in when the caller passed
        // neither argument explicitly — and the pybind layer rejects
        // that case before we get here, so the path is effectively
        // unreachable outside Rust unit tests.
        local.time_ms = if depth.is_some() {
            time_ms
        } else {
            time_ms.or(self.cfg.time_ms)
        };
        search_root(
            &mut self.board,
            &mut self.tt,
            &mut self.ordering,
            &mut self.scratch,
            &local,
        )
    }

    /// Static eval cached on the board.
    #[must_use]
    pub fn cached_eval(&self) -> i32 {
        self.board.cached_eval()
    }

    /// Side that places the next stone.
    #[must_use]
    pub fn to_move(&self) -> Player {
        self.board.to_move()
    }

    /// Cached winner, or `None`.
    #[must_use]
    pub fn winner(&self) -> Option<Player> {
        self.board.winner()
    }

    /// Stones placed so far.
    #[must_use]
    pub fn ply(&self) -> u32 {
        self.board.ply()
    }

    /// Halfmove flag — `0` if next stone starts a fresh turn, `1` if
    /// the same side will play their second stone.
    #[must_use]
    pub fn halfmove(&self) -> u8 {
        self.board.halfmove()
    }

    /// 128-bit Zobrist hash of the current position.
    #[must_use]
    pub fn hash(&self) -> u128 {
        self.board.hash()
    }

    /// Walk the transposition table from the current position, returning
    /// up to `depth` best-move plies. Stops early at the first TT miss
    /// or illegal probe. The board is restored to its starting state
    /// before return.
    ///
    /// # Panics
    ///
    /// Panics only if the internal `undo` rewind disagrees with `place` —
    /// an invariant violation, not a reachable runtime error.
    pub fn find_pv(&mut self, depth: i8) -> Vec<Coord> {
        let max = depth.max(0) as usize;
        if max == 0 {
            return Vec::new();
        }
        let mut pv: Vec<Coord> = Vec::with_capacity(max);
        while pv.len() < max {
            let Some(entry) = self.tt.probe(self.board.hash()) else {
                break;
            };
            // `place` rejects already-occupied or out-of-range coords,
            // so it doubles as the sanity gate on the TT-recorded move.
            if self.board.place(entry.best_move).is_err() {
                break;
            }
            pv.push(entry.best_move);
            if self.board.winner().is_some() {
                break;
            }
        }
        for _ in 0..pv.len() {
            self.board.undo().expect("undo within find_pv must succeed");
        }
        pv
    }

    /// Reset to a fresh game. TT and ordering state are retained (their
    /// own clearing methods are available on the public fields).
    pub fn reset(&mut self) {
        self.board.reset();
    }

    /// Wipe the transposition table. Ordering history is intentionally
    /// preserved — TT scales with positions seen, history is per-game
    /// move-quality memory and outlives a single search.
    pub fn clear_tt(&mut self) {
        self.tt.clear();
    }

    /// Snapshot of the runtime eval overrides held by the underlying
    /// board. Cheap (`Copy`).
    #[must_use]
    pub fn eval_overrides(&self) -> EvalOverrides {
        self.board.eval_overrides()
    }

    /// Install fresh runtime eval overrides. Wipes the transposition
    /// table so search results computed under the old weights cannot
    /// leak into the new run, and forwards to
    /// [`Board::set_eval_overrides`] for the per-board cache
    /// invalidation (Phase-27 `LineContribution` + lazy static eval +
    /// threats). Persists across [`Self::reset`].
    pub fn set_eval_overrides(&mut self, overrides: EvalOverrides) {
        self.board.set_eval_overrides(overrides);
        self.tt.clear();
    }

    /// Diagnostic snapshot of the TT (occupancy + probe/hit/store/
    /// collision counters). Counter fields are populated only when the
    /// engine is built with Cargo feature `tt_stats`; otherwise they
    /// read as zero. See [`crate::tt::TTStatsSnapshot`].
    #[must_use]
    pub fn tt_stats(&self) -> crate::tt::TTStatsSnapshot {
        self.tt.stats()
    }
}

